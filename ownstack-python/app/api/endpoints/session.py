from __future__ import annotations

import time
from collections import defaultdict
from typing import Dict, Tuple
from fastapi import APIRouter, Request

from app.core.errors import APIError, ErrorCodes
from app.core.globals import STATE
from app.tools.git_time import get_time_machine
from app.utils.audit import log_event, AuditEvent

from pydantic import BaseModel

router = APIRouter(prefix="/session", tags=["session"])


# ========== SOTA Phase 74: Rate Limiting & Session Management ==========

class RateLimiter:
    """Simple in-memory rate limiter for API protection."""
    
    def __init__(self, max_requests: int = 30, window_seconds: int = 60):
        self.max_requests = max_requests
        self.window_seconds = window_seconds
        self._requests: Dict[str, list] = defaultdict(list)
    
    def is_allowed(self, client_id: str) -> Tuple[bool, int]:
        """Check if request is allowed. Returns (allowed, seconds_until_reset)."""
        now = time.time()
        window_start = now - self.window_seconds
        
        # Clean old requests
        self._requests[client_id] = [
            ts for ts in self._requests[client_id] if ts > window_start
        ]
        
        if len(self._requests[client_id]) >= self.max_requests:
            oldest = min(self._requests[client_id]) if self._requests[client_id] else now
            reset_in = int(oldest + self.window_seconds - now)
            return False, max(1, reset_in)
        
        self._requests[client_id].append(now)
        return True, 0


class SessionLimiter:
    """Limits concurrent sessions per client."""
    
    MAX_SESSIONS_PER_CLIENT = 5
    _client_sessions: Dict[str, list] = defaultdict(list)
    
    @classmethod
    def can_create_session(cls, client_id: str) -> bool:
        """Check if client can create another session."""
        return len(cls._client_sessions[client_id]) < cls.MAX_SESSIONS_PER_CLIENT
    
    @classmethod
    def register_session(cls, client_id: str, session_id: str):
        """Register a new session for client."""
        if session_id not in cls._client_sessions[client_id]:
            cls._client_sessions[client_id].append(session_id)
    
    @classmethod
    def unregister_session(cls, client_id: str, session_id: str):
        """Unregister a session for client."""
        if session_id in cls._client_sessions[client_id]:
            cls._client_sessions[client_id].remove(session_id)


# Global rate limiter instance
_rate_limiter = RateLimiter(max_requests=30, window_seconds=60)


def _get_client_id(request: Request = None) -> str:
    """Get client identifier from request."""
    if request:
        # Use X-Forwarded-For if behind proxy, otherwise use client host
        forwarded = request.headers.get("X-Forwarded-For")
        if forwarded:
            return forwarded.split(",")[0].strip()
        return request.client.host if request.client else "unknown"
    return "default"


class InitRequest(BaseModel):
    workspace_root: str | None = None


@router.get("/list")
async def list_sessions(request: Request) -> dict:
    # SOTA: Rate limit check
    client_id = _get_client_id(request)
    allowed, reset_in = _rate_limiter.is_allowed(client_id)
    if not allowed:
        raise APIError(429, ErrorCodes.RATE_LIMITED, f"Rate limit exceeded. Retry in {reset_in}s")
    
    sessions = await STATE.session_manager.list_sessions()
    return {"sessions": sessions}


@router.post("/start")
async def start_session(request: Request, payload: InitRequest | None = None) -> dict:
    # SOTA: Rate limit check
    client_id = _get_client_id(request)
    allowed, reset_in = _rate_limiter.is_allowed(client_id)
    if not allowed:
        raise APIError(429, ErrorCodes.RATE_LIMITED, f"Rate limit exceeded. Retry in {reset_in}s")
    
    # SOTA: Session limit check
    if not SessionLimiter.can_create_session(client_id):
        raise APIError(429, ErrorCodes.SESSION_LIMIT_EXCEEDED, 
                      f"Max {SessionLimiter.MAX_SESSIONS_PER_CLIENT} concurrent sessions allowed")
    
    workspace_root = payload.workspace_root if payload else None
    print(f"DEBUG: Starting session with workspace_root: {workspace_root}")
    try:
        session_id, container_id = await STATE.runtime.start_async(workspace_root=workspace_root)
    except Exception as e:
        import traceback
        traceback.print_exc()
        error_msg = str(e)
        # Detect Windows Named Pipe or Connection Refused errors
        if "cannot find the file specified" in error_msg or "No connection could be made" in error_msg:
             raise APIError(503, ErrorCodes.INTERNAL_ERROR, "Docker Service Unavailable. Is Docker Desktop running?", details={"original_error": error_msg})
        raise APIError(500, ErrorCodes.INTERNAL_ERROR, f"Failed to start session: {error_msg}")
    
    # SOTA: Register session for limiting
    SessionLimiter.register_session(client_id, session_id)
    
    await STATE.session_manager.register_session(session_id, container_id)
    log_event(AuditEvent.SESSION_START, session_id=session_id, container_id=container_id)
    return {"session_id": session_id, "container_id": container_id}


@router.post("/{session_id}/stop")
async def stop_session(session_id: str) -> dict:
    container_id = await STATE.session_manager.remove_session(session_id)
    if not container_id:
        raise APIError(404, ErrorCodes.SESSION_NOT_FOUND, "Session not found", session_id=session_id)
    await STATE.runtime.stop_async(container_id)
    log_event(AuditEvent.SESSION_STOP, session_id=session_id, container_id=container_id)
    return {"status": "stopped"}


@router.get("/{session_id}/status")
async def status_session(session_id: str) -> dict:
    container_id = await STATE.session_manager.get_session(session_id)
    if not container_id:
        raise APIError(404, ErrorCodes.SESSION_NOT_FOUND, "Session not found", session_id=session_id)
    status = await STATE.runtime.status(container_id)
    return {"status": status}


@router.post("/{session_id}/restore/{checkpoint_id}")
async def restore_session_checkpoint(
    session_id: str, 
    checkpoint_id: str,
) -> dict:
    """Restores the session to a specific Docker + Git checkpoint."""
    container_id = await STATE.session_manager.get_session(session_id)
    if not container_id:
        raise APIError(404, ErrorCodes.SESSION_NOT_FOUND, "Session not found", session_id=session_id)
    
    tm = get_time_machine(STATE.runtime)
    try:
        success = await tm.rollback(container_id, checkpoint_id)
        if not success:
             raise APIError(500, ErrorCodes.INTERNAL_ERROR, "Rollback failed")
             
        log_event(AuditEvent.SESSION_RESTORE, session_id=session_id, data={"checkpoint_id": checkpoint_id})
        return {"status": "restored", "checkpoint_id": checkpoint_id}
    except ValueError as e:
         raise APIError(404, ErrorCodes.RESOURCE_NOT_FOUND, str(e))
    except Exception as e:
        raise APIError(500, ErrorCodes.INTERNAL_ERROR, str(e))

@router.post("/{session_id}/snapshot")
async def create_session_snapshot(
    session_id: str,
    message: str = "Manual snapshot",
) -> dict:
    """Takes a manual snapshot of the current session state."""
    container_id = await STATE.session_manager.get_session(session_id)
    if not container_id:
        raise APIError(404, ErrorCodes.SESSION_NOT_FOUND, "Session not found", session_id=session_id)
    
    tm = get_time_machine(STATE.runtime)
    snapshot = await tm.snapshot(container_id, message)
    return {
        "snapshot_id": snapshot.id,
        "message": snapshot.message,
        "timestamp": snapshot.timestamp
    }
