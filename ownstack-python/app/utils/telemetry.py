"""Black Box Telemetry Logger with SOTA Tracing.

Logs every agent interaction (prompt, response, tool call, tool result) 
to a persistent local file for audit and debug purposes.

SOTA Phase 79: OpenTelemetry-compatible trace context (shim mode).
SOTA Phase 69: Event Loop Monitoring.
"""
from __future__ import annotations

import json
import logging
import time
import uuid
import contextvars
import asyncio
from contextlib import contextmanager
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, Optional


# SOTA Phase 79: Trace Context for distributed tracing
@dataclass
class TraceContext:
    """OpenTelemetry-compatible trace context (local shim)."""
    trace_id: str = field(default_factory=lambda: uuid.uuid4().hex)
    span_id: str = field(default_factory=lambda: uuid.uuid4().hex[:16])
    parent_span_id: Optional[str] = None
    operation_name: str = "unknown"
    start_time: float = field(default_factory=time.time)
    attributes: Dict[str, Any] = field(default_factory=dict)


# Context variable for active span
_active_span: contextvars.ContextVar[Optional[TraceContext]] = contextvars.ContextVar("active_span", default=None)


def get_current_trace() -> Optional[TraceContext]:
    """Get the current active trace context."""
    return _active_span.get()


@contextmanager
def start_span(operation_name: str, attributes: Optional[Dict[str, Any]] = None):
    """
    SOTA Phase 79: Context manager for creating trace spans.
    
    Usage:
        with start_span("process_command", {"command": "pytest"}) as span:
            # do work
            span.attributes["result"] = "success"
    """
    parent = _active_span.get()
    
    span = TraceContext(
        trace_id=parent.trace_id if parent else uuid.uuid4().hex,
        span_id=uuid.uuid4().hex[:16],
        parent_span_id=parent.span_id if parent else None,
        operation_name=operation_name,
        attributes=attributes or {}
    )
    
    token = _active_span.set(span)
    try:
        yield span
    finally:
        # Log span completion
        duration_ms = int((time.time() - span.start_time) * 1000)
        logging.getLogger("ownstack.trace").debug(
            f"[SPAN] {span.operation_name} trace={span.trace_id} span={span.span_id} "
            f"parent={span.parent_span_id} duration={duration_ms}ms"
        )
        _active_span.reset(token)


class BlackBoxLogger:
    def __init__(self, session_id: str, workspace_root: str):
        self.session_id = session_id
        # Store in .ownstack/telemetry.jsonl
        self.log_dir = Path(workspace_root) / ".ownstack" / "telemetry"
        self.log_dir.mkdir(parents=True, exist_ok=True)
        self.log_file = self.log_dir / f"{session_id}.jsonl"

    def log(self, event_type: str, data: Dict[str, Any]):
        """Append an event to the JSONL log with trace context."""
        entry = {
            "timestamp": time.time(),
            "event": event_type,
            "session_id": self.session_id,
            "data": data
        }
        
        # SOTA Phase 79: Include trace context if active
        trace = get_current_trace()
        if trace:
            entry["trace_id"] = trace.trace_id
            entry["span_id"] = trace.span_id
            if trace.parent_span_id:
                entry["parent_span_id"] = trace.parent_span_id
        
        try:
            with open(self.log_file, "a", encoding="utf-8") as f:
                f.write(json.dumps(entry) + "\n")
        except Exception as e:
            logging.error(f"Telemetry log failed: {e}")

def get_black_box(session_id: str, workspace_root: str) -> BlackBoxLogger:
    return BlackBoxLogger(session_id, workspace_root)


# SOTA Phase 69: Event Loop Monitoring
class EventLoopMonitor:
    """Monitors the asyncio event loop lag."""
    def __init__(self, threshold_ms: float = 50.0):
        self.threshold_ms = threshold_ms
        self._running = False
        self._task = None
        self._last_lag = 0.0

    async def start(self):
        self._running = True
        self._task = asyncio.create_task(self._monitor())
    
    async def stop(self):
        self._running = False
        if self._task:
            self._task.cancel()
            try:
                await self._task
            except asyncio.CancelledError:
                pass
    
    def get_lag(self) -> float:
        return self._last_lag

    async def _monitor(self):
        while self._running:
            start = time.perf_counter()
            await asyncio.sleep(1)
            end = time.perf_counter()
            # Expected 1s, diff is lag + 1s
            elapsed = (end - start)
            lag_ms = (elapsed - 1.0) * 1000
            self._last_lag = lag_ms
            
            if lag_ms > self.threshold_ms:
                logging.getLogger("ownstack.perf").warning(
                    f"High Event Loop Lag detected: {lag_ms:.2f}ms"
                )

# Global Monitor
_loop_monitor: Optional[EventLoopMonitor] = None

async def start_performance_monitoring():
    """Start global performance monitoring."""
    global _loop_monitor
    if not _loop_monitor:
        # P1: Only start if loop is running
        try:
            asyncio.get_running_loop()
            _loop_monitor = EventLoopMonitor()
            await _loop_monitor.start()
        except RuntimeError:
            pass

def get_loop_lag() -> float:
    """Get current event loop lag in ms."""
    return _loop_monitor.get_lag() if _loop_monitor else 0.0
