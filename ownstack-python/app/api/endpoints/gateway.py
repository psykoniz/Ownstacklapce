from fastapi import APIRouter, WebSocket, WebSocketDisconnect
from app.core.globals import STATE
from app.utils.audit import log_event
import json
import asyncio
import time
from typing import Dict, List

import logging
logger = logging.getLogger(__name__)

router = APIRouter()


# ========== SOTA Phase 74: WebSocket Health Management ==========

class ConnectionManager:
    """Manages WebSocket connections with health monitoring."""
    
    def __init__(self):
        self.active_connections: Dict[str, dict] = {}  # conn_id -> {websocket, last_ping, queue_size}
        self.MAX_QUEUE_SIZE = 100  # Backpressure threshold
        self.HEARTBEAT_INTERVAL = 30  # seconds
    
    def register(self, conn_id: str, websocket: WebSocket):
        self.active_connections[conn_id] = {
            "websocket": websocket,
            "last_ping": time.time(),
            "queue_size": 0,
            "connected_at": time.time(),
        }
    
    def unregister(self, conn_id: str):
        self.active_connections.pop(conn_id, None)
    
    def update_ping(self, conn_id: str):
        if conn_id in self.active_connections:
            self.active_connections[conn_id]["last_ping"] = time.time()
    
    def is_backpressured(self, conn_id: str) -> bool:
        """Check if connection should be throttled."""
        conn = self.active_connections.get(conn_id)
        if conn:
            return conn["queue_size"] >= self.MAX_QUEUE_SIZE
        return False
    
    def get_stats(self) -> dict:
        """Get connection statistics."""
        return {
            "active_connections": len(self.active_connections),
            "connections": [
                {
                    "id": cid[:8] + "...",
                    "uptime_seconds": int(time.time() - c["connected_at"]),
                    "last_ping_ago": int(time.time() - c["last_ping"]),
                }
                for cid, c in self.active_connections.items()
            ]
        }


_connection_manager = ConnectionManager()


@router.get("/ws/stats")
async def websocket_stats() -> dict:
    """Get WebSocket connection statistics. SOTA Phase 74."""
    return _connection_manager.get_stats()


@router.websocket("/ws/gateway")
async def gateway_ws(websocket: WebSocket):
    """
    Global WebSocket Gateway protected by API Key.
    SOTA Phase 74: Added heartbeat, backpressure, and connection monitoring.
    """
    import os
    import uuid
    
    api_key = websocket.query_params.get("api_key")
    if not api_key or api_key != os.getenv("OWNSTACK_API_KEY", "os-dev-key"):
        await websocket.close(code=4003) # Forbidden
        return

    conn_id = str(uuid.uuid4())
    logger.info(f"New gateway connection authorized: {conn_id[:8]}")
    await websocket.accept()
    
    # Register connection
    _connection_manager.register(conn_id, websocket)
    logger.info("Gateway connection accepted")
    
    # Callback to send events with backpressure
    async def send_event(event_type: str, data: any):
        try:
            # SOTA: Backpressure check
            if _connection_manager.is_backpressured(conn_id):
                logger.warning(f"Connection {conn_id[:8]} backpressured, dropping event")
                return
                
            await websocket.send_json({
                "type": event_type,
                "payload": data
            })
        except:
            # Connection might be closed
            pass

    # Subscribe to mission manager events
    STATE.mission_manager.subscribe(send_event)
    
    # SOTA: Heartbeat task
    async def heartbeat():
        while conn_id in _connection_manager.active_connections:
            try:
                await websocket.send_json({"type": "heartbeat", "ts": int(time.time())})
                await asyncio.sleep(_connection_manager.HEARTBEAT_INTERVAL)
            except:
                break
    
    heartbeat_task = asyncio.create_task(heartbeat())
    
    try:
        # Keep connection alive and handle incoming control messages
        while True:
            try:
                data = await asyncio.wait_for(
                    websocket.receive_text(), 
                    timeout=60.0  # Increased timeout with heartbeat
                )
                message = json.loads(data)
            except asyncio.TimeoutError:
                # No message received, but heartbeat keeps connection alive
                continue
            except (json.JSONDecodeError, WebSocketDisconnect):
                break
            except Exception as e:
                logger.error(f"Gateway message error: {e}")
                continue
            
            # Handle control messages
            msg_type = message.get("type")
            
            if msg_type == "ping":
                _connection_manager.update_ping(conn_id)
                await websocket.send_json({"type": "pong", "ts": int(time.time())})
            elif msg_type == "pong":
                _connection_manager.update_ping(conn_id)
            # Future: Handle 'cancel_mission', 'pause_mission', etc.

    except WebSocketDisconnect:
        pass
    finally:
        heartbeat_task.cancel()
        _connection_manager.unregister(conn_id)
        STATE.mission_manager.unsubscribe(send_event)
        logger.info(f"Gateway connection closed: {conn_id[:8]}")
