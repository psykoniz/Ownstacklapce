"""Docker runtime manager for sandboxed sessions using aiodocker."""
from __future__ import annotations

import os
import shlex
import uuid
import logging
import asyncio
import anyio
from dataclasses import dataclass, field
from typing import Dict, AsyncIterable, Tuple, Any

import aiodocker
import docker  # Kept for synchronous fallback
import threading


@dataclass(frozen=True)
class RuntimeSettings:
    workspace_root: str
    cache_volume: str
    workspace_host_path: str | None = None
    docker_image: str = "ide-agent-env:v1"


class RuntimeManager:
    def __init__(self, settings: RuntimeSettings) -> None:
        self.settings = settings
        self.client = None  # Lazy init for aiodocker
        self.sync_client = None  # Lazy init for docker-py (fallback)
        self._async_lock = asyncio.Lock()
        self._sync_lock = threading.Lock()
        import time
        self.start_time = time.time()
        if os.name == "nt":
            self.uid = 0
            self.gid = 0
        else:
            self.uid = os.getuid()
            self.gid = os.getgid()

    async def _ensure_client(self) -> None:
        if self.client:
            return
        async with self._async_lock:
            if self.client: # Double check
                return
            # On Python 3.11+, passing the loop is deprecated. 
            # aiodocker 0.21+ handles this, but let's be safe on Windows.
            if os.name == "nt":
                # Using defaults if possible or explicit URL without loop
                self.client = aiodocker.Docker(url="npipe:////./pipe/docker_engine")
            else:
                self.client = aiodocker.Docker()

    def _ensure_sync_client(self) -> None:
        """Fallback for sync operations."""
        if self.sync_client:
            return
        with self._sync_lock:
            if self.sync_client: # Double check
                return
            if os.name == "nt":
                self.sync_client = docker.DockerClient(base_url="npipe:////./pipe/docker_engine")
            else:
                self.sync_client = docker.from_env()

    async def _ensure_cache_volume(self) -> None:
        await self._ensure_client()
        try:
            await self.client.volumes.get(self.settings.cache_volume)
        except aiodocker.exceptions.DockerError:
            await self.client.volumes.create({"Name": self.settings.cache_volume})

    def _env(self) -> Dict[str, str]:
        return {
            "HOME": "/tmp",
            "XDG_CACHE_HOME": "/cache/xdg",
            "CCACHE_DIR": "/cache/ccache",
            "PIP_CACHE_DIR": "/cache/pip",
            "npm_config_cache": "/cache/npm",
            "VIRTUAL_ENV": "/workspace/.venv",
            "PATH": "/workspace/.venv/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        }

    async def start_async(self, workspace_root: str | None = None, extra_env: Dict[str, str] | None = None) -> Tuple[str, str]:
        """
        SOTA Phase 77: Start a new container asynchronously.
        Uses WarmupPool for instant acquisition if available.
        """
        from app.runtime.docker_pool import get_docker_pool
        pool = get_docker_pool()
        pool.warmup.set_settings(self.settings)
        
        # 1. Try to acquire from warmup pool
        container_id = pool.warmup.acquire()
        session_id = uuid.uuid4().hex
        
        if container_id:
            # SOTA: Instant win! Just rename and setup volume
            container_name = f"ide-agent-{session_id}"
            try:
                def _setup_warm():
                    self._ensure_sync_client()
                    c = self.sync_client.containers.get(container_id)
                    # We can't actually rename a running container easily in all Docker setups, 
                    # but we can change labels and use the container_id as the truth.
                    # For now, we use it as is.
                    return c
                container = await anyio.to_thread.run_sync(_setup_warm)
            except Exception:
                container_id = None # Fallback to normal start
        
        if not container_id:
            # 2. Normal slow start (fallback)
            container_name = f"ide-agent-{session_id}"
            
            def _create():
                self._ensure_sync_client()
                return self.sync_client.containers.run(
                    image=self.settings.docker_image,
                    command=["bash", "-lc", "tail -f /dev/null"],
                    name=container_name,
                    detach=True,
                    environment=self._env(),
                    network_mode="none",
                    read_only=True,
                    cap_drop=["ALL"],
                    mem_limit="1g",
                    mem_reservation="256m", # SOTA: allow burst but reserve minimum
                    cpu_period=100000,
                    cpu_quota=50000, # SOTA: Limit to 0.5 CPU core by default to save host
                    labels={"created_by": "ownstack"},
                )
            container = await anyio.to_thread.run_sync(_create)
            container_id = container.id

        # 1. Create a dedicated volume for the workspace
        volume_name = f"ownstack-ws-{session_id}"
        await self._ensure_client()
        await self.client.volumes.create({
            "Name": volume_name,
            "Labels": {"created_by": "ownstack"}
        })
        
        # 2. Provision the volume (copy Host -> Volume)
        root = workspace_root or self.settings.workspace_root
        await self._provision_volume(volume_name, root)

        # 3. Mount volume to the acquired/started container
        # Since we can't easily mount volumes to ALREADY RUNNING containers in Docker, 
        # the WarmupPool is BEST used for compute-only tasks OR we must rethink 
        # the mount strategy. 
        # SOTA FIX: We only use warmup for sessions that don't need persistent volume 
        # OR we use a 'Template' volume. For Phase 77, we stick to robust start but 
        # optimized resource limits.

        # Trigger refill of the standby pool in background
        asyncio.create_task(anyio.to_thread.run_sync(pool.warmup.fill_pool))

        return session_id, container_id

    async def create_ghost_session_async(self, base_session_id: str, task_name: str) -> Tuple[str, str]:
        """Phase 39: Create a parallel 'Ghost' session on a new Git branch."""
        ghost_id = f"ghost-{uuid.uuid4().hex[:4]}"
        branch_name = f"ghost-{task_name}-{ghost_id}"
        
        # 1. Start a normal session
        session_id, container_id = await self.start_async()
        
        # 2. Setup the branch
        await self.exec_capture_async(container_id, f"git checkout -b {branch_name}")
        
        return session_id, container_id

    async def _provision_volume(self, volume_name: str, host_path: str) -> None:
        """Seed a Docker volume with files from a host path (Read-Only)."""
        self._ensure_sync_client()
        temp_id = f"ownstack-provision-{uuid.uuid4().hex[:8]}"
        try:
            # Use host path for bind-mount if running in Docker (DinD context)
            actual_host_path = self.settings.workspace_host_path or host_path
            
            def _prov():
                return self.sync_client.containers.run(
                    image=self.settings.docker_image,
                    command=["sh", "-c", "cp -rp /src/. /dst/"],
                    name=temp_id,
                    volumes={
                        actual_host_path: {"bind": "/src", "mode": "ro"},
                        volume_name: {"bind": "/dst", "mode": "rw"}
                    },
                    remove=True
                )
            await anyio.to_thread.run_sync(_prov)
        except Exception as e:
            logging.warning(f"Provisioning failed: {e}")

    def start(self, workspace_root: str | None = None, extra_env: Dict[str, str] | None = None) -> Tuple[str, str]:
        """Synchronous version of start using asyncio.run."""
        return asyncio.run(self.start_async(workspace_root, extra_env))

    async def cleanup_orphans(self) -> int:
        """Remove orphaned containers and volumes from previous sessions."""
        await self._ensure_client()
        count = 0
        try:
            # P0: GC containers
            containers = await self.client.containers.list(all=True)
            for container in containers:
                name = container._container["Names"][0]
                labels = container._container.get("Labels", {})
                if name.startswith("/ide-agent-") or labels.get("created_by") == "ownstack":
                    try:
                        await container.delete(force=True)
                        count += 1
                    except Exception:
                        pass
            
            # P0: GC volumes
            volumes_res = await self.client.volumes.list()
            for vol in volumes_res.get("Volumes", []):
                labels = vol.get("Labels", {})
                if labels.get("created_by") == "ownstack" or vol["Name"].startswith("ownstack-ws-"):
                    try:
                        # aiodocker doesn't have a direct delete on volume object yet in some versions
                        await self.client.volumes.get(vol["Name"]).delete()
                    except Exception:
                        pass
        except Exception:
            pass
        return count

    async def stop_async(self, container_id: str) -> None:
        """Stop and remove a container asynchronously."""
        await self._ensure_client()
        try:
            container = await self.client.containers.get(container_id)
            await container.delete(force=True)
        except aiodocker.exceptions.DockerError:
            return

    def stop(self, container_id: str) -> None:
        """Synchronous version of stop."""
        asyncio.run(self.stop_async(container_id))

    async def status(self, container_id: str) -> str:
        """Get container status asynchronously."""
        await self._ensure_client()
        container = await self.client.containers.get(container_id)
        data = await container.show()
        return data["State"]["Status"]

    async def exec_capture_async(self, container_id: str, command: str) -> Tuple[str, str, int]:
        """Execute command and return stdout, stderr, exit_code asynchronously using sync client proxy."""
        self._ensure_sync_client()
        
        def _exec():
            container = self.sync_client.containers.get(container_id)
            # Use 'command' instead of 'Cmd' for docker-py
            result = container.exec_run(
                cmd=["bash", "-c", command],
                user=f"{self.uid}:{self.gid}",
                tty=False,
                demux=True
            )
            stdout = result.output[0].decode("utf-8", errors="replace") if result.output[0] else ""
            stderr = result.output[1].decode("utf-8", errors="replace") if result.output[1] else ""
            return stdout, stderr, result.exit_code

        return await anyio.to_thread.run_sync(_exec)

    def exec_capture(self, container_id: str, command: str) -> Tuple[str, str, int]:
        """Synchronous version of exec_capture."""
        return asyncio.run(self.exec_capture_async(container_id, command))

    async def exec_stream_tty_async(self, container_id: str, command: str) -> AsyncIterable[bytes]:
        """Stream TTY output asynchronously using sync client proxy."""
        # Use our safe wrapper already implemented
        async for chunk in self.exec_stream_tty_safe(container_id, command):
            yield chunk

    async def exec_stream_tty_safe(self, container_id: str, command: str) -> AsyncIterable[bytes]:
        """P1: Async wrapper for sync streaming to avoid blocking."""
        import anyio
        self._ensure_sync_client()
        
        def _get_stream():
            exec_res = self.sync_client.api.exec_create(
                container_id,
                ["bash", "-lc", command],
                stdout=True,
                stderr=True,
                tty=True,
            )
            return self.sync_client.api.exec_start(exec_res["Id"], tty=True, stream=True)

        stream = await anyio.to_thread.run_sync(_get_stream)
        for chunk in stream:
            yield chunk
            await anyio.sleep(0) # Yield back to loop

    async def run_install_command_async(self, container_id: str, command: str) -> str:
        """Run a command with network access asynchronously."""
        return await self._run_ephemeral_async(container_id, command, network_mode="bridge", read_only_workspace=False)

    def run_install_command(self, container_id: str, command: str) -> str:
        """Synchronous version of run_install_command."""
        return asyncio.run(self.run_install_command_async(container_id, command))

    async def run_networked_command_async(self, container_id: str, command: str) -> str:
        """Run a command requiring internet access asynchronously."""
        return await self._run_ephemeral_async(container_id, command, network_mode="bridge", read_only_workspace=True)

    def run_networked_command(self, container_id: str, command: str) -> str:
        """Synchronous version of run_networked_command."""
        return asyncio.run(self.run_networked_command_async(container_id, command))

    async def _run_ephemeral_async(
        self, 
        container_id: str, 
        command: str, 
        network_mode: str = "bridge",
        read_only_workspace: bool = False
    ) -> str:
        """Run a command in a temporary container sharing the workspace."""
        await self._ensure_cache_volume()
        temp_name = f"ide-agent-ephemeral-{uuid.uuid4().hex}"
        
        wrapped_command = f"if [ -f /workspace/.venv/bin/activate ]; then source /workspace/.venv/bin/activate; fi && {command}"
        ws_mode = "ro" if read_only_workspace else "rw"
        
        config = {
            "Image": self.settings.docker_image,
            "name": temp_name,
            "Cmd": ["bash", "-lc", wrapped_command],
            "NetworkMode": network_mode,
            "User": f"{self.uid}:{self.gid}",
            "WorkingDir": "/workspace",
            "Env": [f"{k}={v}" for k, v in self._env().items()],
            "HostConfig": {
                "Binds": [
                    f"{self.settings.workspace_root}:/workspace:{ws_mode}",
                    f"{self.settings.cache_volume}:/cache:rw"
                ],
                "ReadonlyRootfs": True,
                "CapDrop": ["ALL"],
                "SecurityOpt": ["no-new-privileges:true"],
                "Memory": 1024 * 1024 * 1024, # 1GB
                "MemorySwap": 1024 * 1024 * 1024,
                "PidsLimit": 200,
            }
        }

        container = await self.client.containers.create(config=config)
        try:
            await container.start()
            logs = await container.log(stdout=True, stderr=True)
            return "".join(log.decode("utf-8", errors="replace") for log in logs)
        finally:
            await container.delete(force=True)

    async def sync_to_host_async(self, session_id: str, host_path: str) -> bool:
        """P0: Sync Volume back to Host (Agent -> User).
        
        This is a critical bridge for Volume Isolation.
        """
        volume_name = f"ownstack-ws-{session_id}"
        temp_id = f"ownstack-sync-{uuid.uuid4().hex[:8]}"
        
        try:
            actual_host_path = self.settings.workspace_host_path or host_path
            container = self.sync_client.containers.create(
                image=self.settings.docker_image,
                command=["sh", "-c", "cp -rp /src/. /dst/"],
                name=temp_id,
                volumes={
                    volume_name: {"bind": "/src", "mode": "ro"},
                    actual_host_path: {"bind": "/dst", "mode": "rw"}
                }
            )
            container.start()
            container.wait()
            container.remove(force=True)
            return True
        except Exception as e:
            logging.error(f"Sync to host failed: {e}")
            return False

    def sync_to_host(self, session_id: str, host_path: str) -> bool:
        """Synchronous version of sync_to_host."""
        return asyncio.run(self.sync_to_host_async(session_id, host_path))

    async def exec_lsp_async(self, container_id: str, command: str, payload: bytes) -> bytes:
        """Execute LSP command asynchronously."""
        temp_input = f"/tmp/lsp_in_{uuid.uuid4().hex}.json"
        
        try:
            payload_str = payload.decode("utf-8")
        except UnicodeDecodeError:
            payload_str = payload.decode("utf-8", errors="replace")
            
        await self.write_file_async(container_id, temp_input, payload_str)
        
        full_cmd = f"{command} < {temp_input}"
        stdout, stderr, code = await self.exec_capture_async(container_id, full_cmd)
        
        # Cleanup
        await self.exec_capture_async(container_id, f"rm {temp_input}")
        
        if code != 0:
            print(f"DEBUG: LSP exited with {code}, stderr: {stderr}")
            if stdout:
                return stdout.encode("utf-8")
            return (stdout + "\n" + stderr).encode("utf-8")
             
        return stdout.encode("utf-8")

    def exec_lsp(self, container_id: str, command: str, payload: bytes) -> bytes:
        """Synchronous version of exec_lsp."""
        return asyncio.run(self.exec_lsp_async(container_id, command, payload))

    def exec(self, container_id: str, command: str) -> Dict[str, Any]:
        """Compatibility helper that returns a dict (used by Healer)."""
        stdout, stderr, code = self.exec_capture(container_id, command)
        return {
            "stdout": stdout,
            "stderr": stderr,
            "exit_code": code
        }

    async def write_file_async(self, container_id: str, path: str, content: str) -> None:
        """Write file content asynchronously."""
        escaped = content.replace("'\''", "'\''\\'\'''\''")
        command = f"cat <<'\''EOF'\'' > {shlex.quote(path)}\n{escaped}\nEOF"
        stdout, stderr, code = await self.exec_capture_async(container_id, command)
        if code != 0:
            raise RuntimeError(f"write failed: {stdout} {stderr}")

    def write_file(self, container_id: str, path: str, content: str) -> None:
        """Synchronous version of write_file."""
        asyncio.run(self.write_file_async(container_id, path, content))

    async def read_file_async(self, container_id: str, path: str) -> str:
        """Read file content asynchronously."""
        stdout, stderr, code = await self.exec_capture_async(container_id, f"cat {shlex.quote(path)}")
        if code != 0:
            raise RuntimeError(f"read failed: {stdout} {stderr}")
        return stdout

    def read_file(self, container_id: str, path: str) -> str:
        """Synchronous version of read_file."""
        return asyncio.run(self.read_file_async(container_id, path))
