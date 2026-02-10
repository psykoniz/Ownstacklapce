"""Structured audit logging for security and debugging.

SOTA Phase 79: OpenTelemetry-compatible trace correlation.
"""
from __future__ import annotations

import json
import logging
import os
import sys
from collections import defaultdict
from datetime import datetime, timezone
from typing import Any, Dict, Optional

from app.utils.telemetry import get_current_trace

# Configure logger
_logger = logging.getLogger("ownstack.audit")
_handler = logging.StreamHandler(sys.stdout)
_handler.setFormatter(logging.Formatter("%(message)s"))
_logger.addHandler(_handler)
_logger.setLevel(logging.INFO)


# SOTA Phase 79: Simple metrics counters
class AuditMetrics:
    """Prometheus-compatible metrics counters for audit events."""
    
    _counters: Dict[str, int] = defaultdict(int)
    
    @classmethod
    def increment(cls, event_type: str):
        cls._counters[event_type] += 1
    
    @classmethod
    def get_metrics(cls) -> Dict[str, int]:
        return dict(cls._counters)
    
    @classmethod
    def reset(cls):
        cls._counters.clear()


class AuditEvent:
    """Represents an auditable event."""
    
    # Event types
    SESSION_START = "session.start"
    SESSION_STOP = "session.stop"
    SESSION_RESTORE = "session.restore"
    COMMAND_EXEC = "command.exec"
    COMMAND_DENIED = "command.denied"
    FILE_READ = "file.read"
    FILE_WRITE = "file.write"
    LSP_REQUEST = "lsp.request"
    AGENT_STEP = "agent.step"
    AGENT_TOOL_CALL = "agent.tool_call"
    AGENT_ACTION = "agent.action"
    AUTH_SUCCESS = "auth.success"
    AUTH_FAILURE = "auth.failure"
    POLICY_CHECK = "policy.check"
    ERROR = "error"


def log_event(
    event_type: str,
    session_id: Optional[str] = None,
    container_id: Optional[str] = None,
    data: Optional[Dict[str, Any]] = None,
    error: Optional[str] = None,
) -> None:
    """
    Log a structured audit event with trace correlation.
    
    Format: JSON for easy parsing by log aggregators.
    """
    # SOTA Phase 79: Increment metrics
    AuditMetrics.increment(event_type)
    
    event = {
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "event": event_type,
    }
    
    # SOTA Phase 79: Include trace context if active
    trace = get_current_trace()
    if trace:
        event["trace_id"] = trace.trace_id
        event["span_id"] = trace.span_id
    
    if session_id:
        event["session_id"] = session_id
    if container_id:
        event["container_id"] = container_id
    if data:
        event["data"] = data
    if error:
        event["error"] = error
    
    event_json = json.dumps(event)
    _logger.info(event_json)
    # Maintain print for capsys-based tests until refactored
    if os.getenv("OWNSTACK_TESTING") == "1":
        print(event_json)


def log_command(
    session_id: str,
    command: str,
    decision: str,
    exit_code: Optional[int] = None,
) -> None:
    """Log a command execution event."""
    data = {"command": command, "decision": decision}
    if exit_code is not None:
        data["exit_code"] = exit_code
    
    event_type = AuditEvent.COMMAND_EXEC if decision == "ALLOW" else AuditEvent.COMMAND_DENIED
    log_event(event_type, session_id=session_id, data=data)


def log_file_operation(
    session_id: str,
    operation: str,  # "read" or "write"
    path: str,
    success: bool,
    error: Optional[str] = None,
) -> None:
    """Log a file operation event."""
    event_type = AuditEvent.FILE_READ if operation == "read" else AuditEvent.FILE_WRITE
    log_event(
        event_type,
        session_id=session_id,
        data={"path": path, "success": success},
        error=error,
    )


def log_agent_step(
    session_id: str,
    step: int,
    tool_name: Optional[str] = None,
    success: bool = True,
    error: Optional[str] = None,
) -> None:
    """Log an agent step event."""
    data = {"step": step, "success": success}
    if tool_name:
        data["tool"] = tool_name
    
    log_event(AuditEvent.AGENT_STEP, session_id=session_id, data=data, error=error)
