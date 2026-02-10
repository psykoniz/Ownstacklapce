"""Time Machine Module for Code Rollback.

Implements a safe 'Time Travel' mechanism using Git.
Allows taking snapshots of code state and restoring them instantly.
This is lighter and safer than Docker checkpointing for development tasks.
"""
from __future__ import annotations

import logging
import uuid
from dataclasses import dataclass
from typing import List, Optional

from app.core.globals import STATE

logger = logging.getLogger(__name__)


@dataclass
class TimeSnapshot:
    """A point in time for the codebase."""
    id: str
    message: str
    git_hash: str
    timestamp: float


class TimeMachine:
    """
    Manages code snapshots and rollbacks.
    
    Uses the underlying git repository of the project to tag states.
    """
    
    def __init__(self, runtime: object):
        self.runtime = runtime
        self.snapshots: List[TimeSnapshot] = []
        
    async def snapshot(self, container_id: str, message: str) -> TimeSnapshot:
        """Create a new snapshot of the current state."""
        # Config git if needed
        await self._ensure_git_config(container_id)
        
        # Commit all changes
        # Use -m with message and ensure we are on a branch
        cmd = f'git add . && git commit --allow-empty -m "OwnStack Snapshot: {message}"'
        stdout, stderr, code = await self.runtime.exec_capture_async(container_id, cmd)
        
        if code != 0:
            # If clean working tree, it's fine
            if "nothing to commit" in stderr:
                logger.info("Nothing to commit, creating snapshot from current state")
            # If identity unknown, it's a real error (despite our efforts)
            elif "identity unknown" in stderr:
                 # Last ditch effort
                 await self._ensure_git_config(container_id)
                 # Retry once? For now raise but clearer
                 raise RuntimeError(f"Git Identity Unknown, config failed: {stderr}")
            else:
                logger.error(f"Snapshot commit failed (code {code}): {stderr}")
                raise RuntimeError(f"Failed to create snapshot commit: {stderr}")
        
        # Get hash
        stdout, stderr, code = await self.runtime.exec_capture_async(container_id, "git rev-parse HEAD")
        if code != 0:
            logger.error(f"Failed to get git hash after commit: {stderr}")
            raise RuntimeError(f"Could not get git hash: {stderr}")
            
        git_hash = stdout.strip()
        
        import time
        snapshot = TimeSnapshot(
            id=str(uuid.uuid4())[:8],
            message=message,
            git_hash=git_hash,
            timestamp=time.time()
        )
        self.snapshots.append(snapshot)
        
        # P2: Optional Docker commit for deep snapshots
        if os.getenv("OWNSTACK_DEEP_SNAPSHOT") == "true":
            try:
                # This requires sync client for now as per current manager
                await self.runtime.exec_capture_async(container_id, f"docker commit {container_id} ownstack-snap:{snapshot.id}")
                logger.info(f"Deep snapshot (docker commit) created for {snapshot.id}")
            except Exception as e:
                logger.warning(f"Deep snapshot failed: {e}")

        logger.info(f"Created snapshot {snapshot.id} -> {git_hash[:8]}")
        return snapshot

    async def rollback(self, container_id: str, snapshot_id: str) -> bool:
        """Rollback code to a specific snapshot."""
        target = next((s for s in self.snapshots if s.id == snapshot_id), None)
        if not target:
            raise ValueError(f"Snapshot {snapshot_id} not found")
        
        # Hard reset to the hash
        cmd = f"git reset --hard {target.git_hash}"
        stdout, stderr, code = await self.runtime.exec_capture_async(container_id, cmd)
        
        if code != 0:
            logger.error(f"Rollback failed: {stderr}")
            return False
            
        # Clean untracked files to be pristine
        await self.runtime.exec_capture_async(container_id, "git clean -fd")
        
        # Re-install dependencies if requirements.txt changed
        # (Naive check: we could diff requirements.txt, but for now we just log)
        logger.info(f"Rolled back to {target.message} ({target.git_hash})")
        return True

    async def _ensure_git_config(self, container_id: str):
        """Ensure git is configured in the container."""
        # Always set config to avoid 'identity unknown' errors
        await self.runtime.exec_capture_async(container_id, 'git config user.name "OwnStack Agent"')
        await self.runtime.exec_capture_async(container_id, 'git config user.email "agent@ownstack.ai"')
            
        # Check if repo exists
        _, _, code = await self.runtime.exec_capture_async(container_id, "git rev-parse --is-inside-work-tree")
        if code != 0:
            logger.info("Initializing new git repository in container")
            await self.runtime.exec_capture_async(container_id, "git init -b main")
            # Create a first commit if empty to avoid HEAD issues
            await self.runtime.exec_capture_async(container_id, "touch .ownstack_init && git add .ownstack_init")
            await self.runtime.exec_capture_async(container_id, 'git commit -m "Initial commit"')


# Singleton
_time_machine: Optional[TimeMachine] = None

def get_time_machine(runtime: object) -> TimeMachine:
    global _time_machine
    if not _time_machine:
        _time_machine = TimeMachine(runtime)
    return _time_machine
