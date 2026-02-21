from fastapi import Depends, FastAPI, WebSocket, WebSocketDisconnect
from fastapi.middleware.cors import CORSMiddleware

from app.api.endpoints.session import router as session_router
from app.api.endpoints.tools import router as tools_router
from app.api.endpoints.webhooks import router as webhooks_router
from app.api.endpoints.monitoring import router as monitoring_router
from app.api.endpoints.missions import router as missions_router
from app.api.endpoints.gateway import router as gateway_router
from app.core.auth import verify_api_key, is_auth_enabled
from app.core.errors import APIError, api_error_handler
from app.core.globals import STATE
from app.utils.audit import log_event, AuditEvent
import logging

app = FastAPI(title="Ownstack IDE Agent", version="1.0.0")

app.add_middleware(
    CORSMiddleware,
    allow_origins=["http://localhost:3000", "http://localhost:5173", "app.ownstack.ai"],
    allow_methods=["*"],
    allow_headers=["*"],
    allow_credentials=True,
)

# Register custom error handler
app.add_exception_handler(APIError, api_error_handler)

# Apply auth middleware to protected routes
app.include_router(session_router, dependencies=[Depends(verify_api_key)])
app.include_router(tools_router, dependencies=[Depends(verify_api_key)])
app.include_router(monitoring_router, dependencies=[Depends(verify_api_key)])
app.include_router(missions_router, dependencies=[Depends(verify_api_key)])
app.include_router(gateway_router) # Gateway is public/monitored (auth can be added inside)
app.include_router(webhooks_router)  # Webhooks use HMAC, not API key


@app.on_event("startup")
async def startup_event():
    # Cleanup orphaned containers from previous runs
    cleaned = 0
    try:
        cleaned = await STATE.runtime.cleanup_orphans()
    except Exception as e:
        logging.warning(f"Docker cleanup skipped (Docker unavailable): {e}")

    # CRITIQUE: Increase default thread pool size (default 40) to prevent starvation
    # when many concurrent agents run blocking Docker operations.
    import anyio
    try:
        limiter = anyio.to_thread.current_default_thread_limiter()
        limiter.total_tokens = 100
        thread_limit_msg = "Thread pool increased to 100"
    except Exception as e:
        thread_limit_msg = f"Failed to increase thread pool: {e}"

    log_event("server.startup", data={
        "version": "1.0.0",
        "auth_enabled": is_auth_enabled(),
        "orphans_removed": cleaned,
        "thread_config": thread_limit_msg
    })


@app.get("/")
def health() -> dict:
    return {"status": "ok", "auth_enabled": is_auth_enabled(), "version": "1.0.0"}



@app.websocket("/ws/session/{session_id}/terminal")
async def terminal_ws(
    websocket: WebSocket, 
    session_id: str, 
    run_id: str,
    api_key: str | None = None,
) -> None:
    # Authenticate via query param (standard for WebSocket)
    if is_auth_enabled():
        import os
        expected_key = os.getenv("OWNSTACK_API_KEY", "")
        if not api_key or api_key != expected_key:
            await websocket.close(code=1008, reason="Invalid or missing API key")
            log_event(AuditEvent.AUTH_FAILURE, session_id=session_id, data={"reason": "ws_invalid_key"})
            return
    
    try:
        await websocket.accept()
        log_event(AuditEvent.SESSION_START, session_id=session_id, data={"type": "websocket_terminal"})
        
        container_id = await STATE.session_manager.get_session(session_id)
        if not container_id:
            await websocket.send_text("session not found")
            await websocket.close(code=1008)
            return

        command = await STATE.session_manager.get_command(run_id)
        if not command:
            await websocket.send_text("run_id not found")
            await websocket.close(code=1008)
            return

        async for chunk in STATE.runtime.exec_stream_tty_async(container_id, command):
            await websocket.send_bytes(chunk)
            
    except WebSocketDisconnect:
        # Normal disconnection
        pass
    except Exception as e:
        # Capture unexpected errors to prevent zombie threads
        print(f"ERROR: WebSocket crash: {e}")
        try:
            await websocket.close(code=1011) # Internal Error
            log_event(AuditEvent.SYSTEM_ERROR, session_id=session_id, data={"error": str(e), "context": "websocket"})
        except:
            pass
    finally:
        await STATE.session_manager.pop_command(run_id)
        # Ensure cleanup
        try:
            pass # websocket cleanup handled by runtime or already closed
        except:
            pass

