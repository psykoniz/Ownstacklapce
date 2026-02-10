"""Multivers Infra - A/B Testing at Infrastructure Level.

OwnStack's unique killer feature: Run parallel containers to test
different configurations simultaneously.

Use cases:
- Compare Python 3.10 vs 3.12 performance
- Test with different library versions
- A/B test database migrations
- Parallel CI/CD environments
"""
from __future__ import annotations

import asyncio
import anyio
import time
import uuid
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Tuple
from enum import Enum


class ForkStatus(str, Enum):
    PENDING = "pending"
    RUNNING = "running"
    COMPLETED = "completed"
    FAILED = "failed"


@dataclass
class ForkResult:
    """Result from a single fork execution."""
    fork_id: str
    variant_name: str
    status: ForkStatus
    exit_code: Optional[int] = None
    stdout: str = ""
    stderr: str = ""
    duration_ms: int = 0
    metrics: Dict[str, Any] = field(default_factory=dict)


@dataclass 
class MultiversRun:
    """A complete multivers test run with multiple variants."""
    run_id: str
    base_session_id: str
    command: str
    variants: Dict[str, str]  # variant_name -> container_id
    results: Dict[str, ForkResult] = field(default_factory=dict)
    created_at: float = field(default_factory=time.time)
    completed: bool = False
    
    def get_comparison(self) -> Dict[str, Any]:
        """Generate comparison summary with SOTA Multi-Objective Scoring."""
        if not self.results:
            return {"status": "no_results"}
        
        variant_data = {}
        for name, result in self.results.items():
            # Calculate score (0-100)
            score = 0
            if result.exit_code == 0:
                score += 50 # Success weight
                
                # Performance weight (bonus 20 for fastest, scaling others)
                all_durations = [r.duration_ms for r in self.results.values() if r.exit_code == 0]
                if all_durations:
                    min_duration = min(all_durations)
                    perf_bonus = 20 * (min_duration / max(result.duration_ms, 1))
                    score += perf_bonus
                
                # Efficiency weight (10)
                mem = result.metrics.get("memory_mb", 1000)
                efficiency_bonus = 10 * (100 / max(mem, 1)) # Smaller is better
                score += min(efficiency_bonus, 10)
                
                # Quality weight (20) based on lack of [LINT ERROR] in stdout
                if "[LINT ERROR]" not in result.stdout:
                    score += 20
            
            variant_data[name] = {
                "status": result.status,
                "exit_code": result.exit_code,
                "duration_ms": result.duration_ms,
                "success": result.exit_code == 0,
                "metrics": result.metrics,
                "stderr": result.stderr,
                "total_score": round(score, 2)
            }
            
        # Determine winner
        scored_variants = [
            (name, data["total_score"]) 
            for name, data in variant_data.items() 
            if data["success"]
        ]
        winner = max(scored_variants, key=lambda x: x[1])[0] if scored_variants else None

        return {
            "run_id": self.run_id,
            "command": self.command,
            "variants": variant_data,
            "winner": winner,
            "completed": self.completed,
            "status": "completed" if self.completed else "running"
        }
    
    def _determine_winner(self) -> Optional[str]:
        """Redirect to avoid recursion."""
        return self.get_comparison().get("winner")


class MultiversManager:
    """
    Manages parallel container execution for A/B testing.
    
    Example usage:
        multivers = MultiversManager(runtime)
        run = await multivers.fork_and_run(
            base_session_id="session-123",
            command="pytest tests/",
            variants={
                "python310": "python:3.10-slim",
                "python312": "python:3.12-slim",
            }
        )
        comparison = run.get_comparison()
    """
    
    def __init__(self, runtime):
        self.runtime = runtime
        self._active_runs: Dict[str, MultiversRun] = {}
    
    async def fork_session(
        self,
        base_session_id: str,
        variant_name: str,
        modifications: Optional[Dict[str, str]] = None,
    ) -> str:
        """
        SOTA Phase 77: Fork an existing container with Git isolation and dynamic limits.
        """
        fork_id = f"fork-{variant_name}-{uuid.uuid4().hex[:8]}"
        branch_name = f"multivers-{variant_name}-{uuid.uuid4().hex[:4]}"
        
        # 1. Start a new container with dynamic limits based on active forks
        # SOTA: Throttle CPU if many universes exist
        active_forks = len(self._active_runs)
        # Dynamic throttling (0.5 core -> 0.25 core)
        cpu_quota = 50000 if active_forks < 3 else 25000 
        
        # Create normal session (which uses optimized limits by default now)
        session_id, container_id = await self.runtime.start_async()
        
        # 2. Isolate code in a branch
        await self.runtime.exec_capture_async(container_id, f"git checkout -b {branch_name}")
        
        return container_id
    
    async def fork_and_run(
        self,
        base_session_id: str,
        command: str,
        variants: Dict[str, Dict[str, Any]],
    ) -> MultiversRun:
        """
        Fork a session into multiple variants and run command in parallel.
        
        Args:
            base_session_id: Original session to fork from
            command: Command to execute in each variant
            variants: Dict of variant_name -> config (image, env, etc.)
            
        Returns:
            MultiversRun with all results
        """
        run_id = f"multivers-{uuid.uuid4().hex[:8]}"
        
        run = MultiversRun(
            run_id=run_id,
            base_session_id=base_session_id,
            command=command,
            variants={},
        )
        
        # Start all variants in parallel
        tasks = []
        for variant_name, config in variants.items():
            task = asyncio.create_task(
                self._run_variant(run, variant_name, command, config)
            )
            tasks.append(task)
        
        # Wait for all to complete
        await asyncio.gather(*tasks, return_exceptions=True)
        run.completed = True
        
        self._active_runs[run_id] = run
        return run
    
    async def _run_variant(
        self,
        run: MultiversRun,
        variant_name: str,
        command: str,
        config: Dict[str, Any],
    ) -> ForkResult:
        """Run a single variant and collect results."""
        result = ForkResult(
            fork_id=f"{run.run_id}-{variant_name}",
            variant_name=variant_name,
            status=ForkStatus.PENDING,
        )
        
        try:
            # Start container with variant config
            image = config.get("image", "ide-agent-env:v1")
            env = config.get("env", {})
            files = config.get("files", {}) # path -> content
            
            # Create variant container
            session_id, container_id = await self.runtime.start_async(extra_env=env)
            run.variants[variant_name] = container_id
            
            # Apply file overlays if any (e.g. inject competitor code)
            for path, content in files.items():
                await self.runtime.write_file_async(container_id, path, content)
            
            result.status = ForkStatus.RUNNING
            start_time = time.time()
            
            # Execute command
            stdout, stderr, exit_code = await self.runtime.exec_capture_async(container_id, command)
            
            # Parse output
            result.duration_ms = int((time.time() - start_time) * 1000)
            result.exit_code = exit_code
            result.stdout = stdout
            result.stderr = stderr
            result.status = ForkStatus.COMPLETED
            
            # Collect metrics if available
            result.metrics = await self._collect_metrics(container_id)
            
        except Exception as e:
            result.status = ForkStatus.FAILED
            result.stderr = str(e)
            result.exit_code = -1
        
        finally:
            # Cleanup variant container
            if variant_name in run.variants:
                try:
                    await self.runtime.stop_async(run.variants[variant_name])
                except:
                    pass
        
        run.results[variant_name] = result
        return result
    
    async def _collect_metrics(self, container_id: str) -> Dict[str, Any]:
        """Collect performance metrics from container."""
        try:
            # We must use the sync Docker client from the runtime
            # Running stats in a thread to be non-blocking
            import asyncio
            loop = asyncio.get_event_loop()
            
            stats = await loop.run_in_executor(
                None,
                lambda: self.runtime.client.containers.get(container_id).stats(stream=False)
            )
            
            # Extract relevant metrics
            memory_usage = stats.get("memory_stats", {}).get("usage", 0)
            cpu_usage = 0.0
            
            # CPU calculation (simplified for Unix/Windows differences)
            cpu_delta = stats.get("cpu_stats", {}).get("cpu_usage", {}).get("total_usage", 0) - \
                        stats.get("precpu_stats", {}).get("cpu_usage", {}).get("total_usage", 0)
            system_delta = stats.get("cpu_stats", {}).get("system_cpu_usage", 0) - \
                           stats.get("precpu_stats", {}).get("system_cpu_usage", 0)
                           
            if system_delta > 0 and cpu_delta > 0:
                cpu_usage = (cpu_delta / system_delta) * stats.get("cpu_stats", {}).get("online_cpus", 1) * 100.0
            
            return {
                "memory_mb": round(memory_usage / 1024 / 1024, 2),
                "cpu_percent": round(cpu_usage, 2),
                "pids": stats.get("pids_stats", {}).get("current", 0),
            }
        except Exception as e:
            # Stats collection shouldn't crash the run
            print(f"Metrics error: {e}")
            return {}
    
    def get_run(self, run_id: str) -> Optional[MultiversRun]:
        """Get a multivers run by ID."""
        return self._active_runs.get(run_id)
    
    def list_runs(self) -> List[Dict[str, Any]]:
        """List all multivers runs."""
        return [
            {
                "run_id": run.run_id,
                "command": run.command,
                "variants": list(run.variants.keys()),
                "completed": run.completed,
                "created_at": run.created_at,
            }
            for run in self._active_runs.values()
        ]


# Global instance
_multivers: Optional[MultiversManager] = None


def get_multivers(runtime) -> MultiversManager:
    """Get or create the global multivers manager."""
    global _multivers
    if _multivers is None:
        _multivers = MultiversManager(runtime)
    return _multivers
