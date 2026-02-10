from __future__ import annotations
from fastapi import APIRouter, WebSocket, WebSocketDisconnect, BackgroundTasks
from app.core.errors import APIError, ErrorCodes
from app.core.globals import STATE
from app.missions.models import Mission, MissionStatus, WorkerType, MissionSpec
from app.missions.openclaw.orchestrator import OpenClawOrchestrator
from app.missions.compiler import MissionCompiler
from pydantic import BaseModel
import logging
import asyncio

router = APIRouter(prefix="/missions", tags=["missions"])
logger = logging.getLogger(__name__)

class MissionCreateRequest(BaseModel):
    title: str
    description: str
    worker_type: WorkerType = WorkerType.CLAUDE_CODE

@router.post("/enqueue")
async def enqueue_mission(
    payload: MissionCreateRequest,
    background_tasks: BackgroundTasks
) -> dict:
    """Create a new mission and start it in the background."""
    if not STATE.mission_manager:
        raise APIError(500, ErrorCodes.INTERNAL_ERROR, "Mission manager not initialized")
    
    # Note: We skip the formal compilation phase in the direct enqueue for backward compatibility,
    # but in the new flow, we should call /compile first.
    mission = await STATE.mission_manager.create_mission(
        title=payload.title,
        description=payload.description,
        worker_type=payload.worker_type
    )
    
    # Start the orchestrator in the background
    orchestrator = OpenClawOrchestrator(STATE.mission_manager)
    background_tasks.add_task(orchestrator.run_mission, mission.id)
    
    return {"mission_id": mission.id, "status": mission.status}

class MissionCreateWithSpecRequest(BaseModel):
    title: str
    description: str # Redundant if in spec objectives? No, useful for human summary
    spec: MissionSpec
    worker_type: WorkerType = WorkerType.OWNSTACK_AGENT

@router.post("/enqueue_with_spec")
async def enqueue_mission_with_spec(
    payload: MissionCreateWithSpecRequest,
    background_tasks: BackgroundTasks
) -> dict:
    """Create a new mission FROM a compiled Spec and start execution."""
    if not STATE.mission_manager:
        raise APIError(500, ErrorCodes.INTERNAL_ERROR, "Mission manager not initialized")
        
    # 1. Create base mission
    mission = await STATE.mission_manager.create_mission(
        title=payload.title,
        description=payload.description,
        worker_type=payload.worker_type
    )
    
    # 2. Attach Spec (The Contract)
    mission.set_spec(payload.spec)
    # Also save to storage? create_mission saves initial state. 
    # We need to save the spec update.
    # In-memory manager updates ref, but for persistence we might need explicit save.
    # Assuming in-memory for now or auto-save in manager (it's in-memory in globals.py).
    
    # 3. Start Orchestrator
    orchestrator = OpenClawOrchestrator(STATE.mission_manager)
    background_tasks.add_task(orchestrator.run_mission, mission.id)
    
    return {"mission_id": mission.id, "status": mission.status}

@router.post("/compile")
async def compile_mission(payload: MissionCreateRequest) -> MissionSpec:
    """Preview the MissionSpec contract without starting execution."""
    if not STATE.agent_provider:
        raise APIError(500, ErrorCodes.INTERNAL_ERROR, "No agent provider available for compilation")
    
    compiler = MissionCompiler(STATE.agent_provider, STATE.SETTINGS.workspace_root)
    spec = await compiler.compile_prompt(payload.description)
    return spec

@router.get("/list")
async def list_missions(
    limit: int = 20,
    offset: int = 0,
    status: str | None = None,
) -> dict:
    """
    List missions with pagination and optional status filter.
    SOTA Phase 74: Pagination support.
    """
    if not STATE.mission_manager:
        return {"missions": [], "total": 0, "limit": limit, "offset": offset}
    
    all_missions = await STATE.mission_manager.list_missions()
    
    # SOTA: Filter by status if provided
    if status:
        try:
            target_status = MissionStatus(status)
            all_missions = [m for m in all_missions if m.status == target_status]
        except ValueError:
            pass  # Invalid status, ignore filter
    
    # SOTA: Apply pagination
    total = len(all_missions)
    paginated = all_missions[offset:offset + limit]
    
    return {
        "missions": [m.model_dump() for m in paginated],
        "total": total,
        "limit": limit,
        "offset": offset,
        "has_more": offset + limit < total,
    }

@router.get("/{mission_id}")
async def get_mission(mission_id: str) -> dict:
    mission = STATE.mission_manager.get_mission(mission_id)
    if not mission:
        raise APIError(404, ErrorCodes.RESOURCE_NOT_FOUND, "Mission not found")
    return mission.model_dump()

@router.websocket("/ws/{mission_id}")
async def mission_ws(websocket: WebSocket, mission_id: str):
    """WebSocket for real-time mission updates."""
    await websocket.accept()
    
    # Simple pub/sub simulation
    # In a real system, we'd use an event bus or Redis
    last_event_idx = 0
    
    try:
        while True:
            mission = STATE.mission_manager.get_mission(mission_id)
            if not mission:
                await websocket.send_json({"error": "Mission not found"})
                break
                
            # Send new events
            if len(mission.events) > last_event_idx:
                for i in range(last_event_idx, len(mission.events)):
                    await websocket.send_json(mission.events[i].dict())
                last_event_idx = len(mission.events)
                
            # If mission is finished, we could close but let's keep it open for final review
            if mission.status in [MissionStatus.COMPLETED, MissionStatus.FAILED, MissionStatus.CANCELLED]:
                # Send one last check of events and wait a bit before closing or just stay open
                pass
                
            await asyncio.sleep(0.5) # Poll for new events
            
    except WebSocketDisconnect:
        pass
    except Exception as e:
        logger.error(f"WebSocket error for mission {mission_id}: {e}")
        try:
            await websocket.close()
        except:
            pass
