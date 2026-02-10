"""InfraSense - Infrastructure Awareness Module.

Gives OwnStack the ability to "feel" its environment:
- Monitor container CPU/RAM/IO usage
- Detect performance bottlenecks
- Propose optimizations based on resource usage

True Infrastructure-Aware AI.
"""
from __future__ import annotations

import asyncio
import time
from dataclasses import dataclass, field
from typing import Dict, List, Optional
import docker.errors


@dataclass
class ContainerStats:
    """Real-time stats for a container with SOTA metrics."""
    container_id: str
    cpu_percent: float = 0.0
    memory_usage_mb: float = 0.0
    memory_limit_mb: float = 0.0
    memory_percent: float = 0.0
    # SOTA Phase 77: IO and Network
    io_read_mb: float = 0.0
    io_write_mb: float = 0.0
    net_rx_mb: float = 0.0
    net_tx_mb: float = 0.0
    timestamp: float = field(default_factory=time.time)
    
    @property
    def is_stressed(self) -> bool:
        """Heuristic for high load."""
        return self.cpu_percent > 80.0 or self.memory_percent > 85.0

@dataclass
class InfraHealth:
    """Overall system health with aggregate SOTA metrics."""
    active_containers: int
    total_cpu_percent: float
    total_memory_mb: float
    total_io_mb: float = 0.0
    stressed_containers: List[str] = field(default_factory=list)
    warnings: List[str] = field(default_factory=list)


class InfraSense:
    """
    Monitor and analyze infrastructure performance.
    
    Usage:
        sense = InfraSense(docker_client)
        stats = await sense.get_stats(container_id)
        if stats.is_stressed:
            await sense.suggest_optimization(container_id)
    """
    
    def __init__(self, docker_client):
        self.client = docker_client
    
    async def get_container_stats(self, container_id: str) -> Optional[ContainerStats]:
        """Get real-time stats for a specific container."""
        try:
            container = self.client.containers.get(container_id)
            # SOTA Phase 69: Fix missing await on coroutine
            stats = await container.stats(stream=False)
            
            return self._parse_stats(container_id, stats)
        except docker.errors.NotFound:
            return None
        except Exception:
            return None
    
    async def get_system_health(self) -> InfraHealth:
        """Scan all active agents for health issues."""
        if not self.client:
            return InfraHealth(
                active_containers=0,
                total_cpu_percent=0.0,
                total_memory_mb=0.0,
                stressed_containers=[],
                warnings=["Docker client not available"],
            )
        active = await self.client.containers.list(filters={"name": "ide-agent-"})
        
        info = InfraHealth(
            active_containers=len(active),
            total_cpu_percent=0.0,
            total_memory_mb=0.0,
            stressed_containers=[],
            warnings=[],
        )
        
        for container in active:
            stats = await self.get_container_stats(container.id)
            if stats:
                info.total_cpu_percent += stats.cpu_percent
                info.total_memory_mb += stats.memory_usage_mb
                
                if stats.is_stressed:
                    info.stressed_containers.append(container._container.get("Name", "unknown"))
                    info.warnings.append(
                        f"High load on container: CPU {stats.cpu_percent:.1f}%, MEM {stats.memory_percent:.1f}%"
                    )
        
        return info
    
    def _parse_stats(self, container_id: str, raw: dict) -> ContainerStats:
        """Parse raw Docker stats into meaningful SOTA metrics."""
        # CPU calc
        try:
            cpu_delta = raw["cpu_stats"]["cpu_usage"]["total_usage"] - \
                        raw["precpu_stats"]["cpu_usage"]["total_usage"]
            system_delta = raw["cpu_stats"]["system_cpu_usage"] - \
                           raw["precpu_stats"]["system_cpu_usage"]
        except KeyError:
            cpu_delta = 0
            system_delta = 0
        
        cpu_percent = 0.0
        if system_delta > 0 and cpu_delta > 0:
            cpu_percent = (cpu_delta / system_delta) * raw["cpu_stats"]["online_cpus"] * 100.0
        
        # Memory calc
        mem_usage = raw["memory_stats"].get("usage", 0)
        mem_limit = raw["memory_stats"].get("limit", 0)
        mem_percent = (mem_usage / mem_limit) * 100.0 if mem_limit > 0 else 0.0
        
        # SOTA Phase 77: IO calc
        io_read = 0.0
        io_write = 0.0
        try:
            for entry in raw.get("blkio_stats", {}).get("io_service_bytes_recursive", []) or []:
                if entry["op"] == "Read":
                    io_read += entry["value"]
                elif entry["op"] == "Write":
                    io_write += entry["value"]
        except Exception:
            pass

        # SOTA Phase 77: Network calc
        net_rx = 0.0
        net_tx = 0.0
        try:
            for net in raw.get("networks", {}).values():
                net_rx += net["rx_bytes"]
                net_tx += net["tx_bytes"]
        except Exception:
            pass

        return ContainerStats(
            container_id=container_id,
            cpu_percent=cpu_percent,
            memory_usage_mb=mem_usage / 1024 / 1024,
            memory_limit_mb=mem_limit / 1024 / 1024,
            memory_percent=mem_percent,
            io_read_mb=io_read / 1024 / 1024,
            io_write_mb=io_write / 1024 / 1024,
            net_rx_mb=net_rx / 1024 / 1024,
            net_tx_mb=net_tx / 1024 / 1024,
        )


# Global instance
_sense: Optional[InfraSense] = None


def get_sense(runtime) -> InfraSense:
    """Get global InfraSense instance."""
    global _sense
    if _sense is None:
        _sense = InfraSense(runtime.client)
    return _sense
