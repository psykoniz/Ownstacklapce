from __future__ import annotations

from fastapi import APIRouter, Query, HTTPException, Depends
from fastapi.responses import StreamingResponse
import json
import asyncio
from pydantic import BaseModel
from typing import Optional, List, Dict, Any

from app.core.errors import APIError, ErrorCodes
from app.core.globals import STATE
# from app.tools.docs.context7_stub import fetch_and_cache  # Removed: function missing
from app.tools.lsp import definitions, references, rename
from app.tools.repomap_runner import generate_repomap_v2
from app.utils.ids import new_id
from app.core.policies import check_command
from app.utils.path_safety import validate_write_path, validate_read_path, PathSafetyError
from app.tools.git_time import get_time_machine
from app.utils.audit import log_command, log_event, AuditEvent
import logging

# Set up local logger for non-audit debug info
logger = logging.getLogger(__name__)

router = APIRouter(prefix="/tools", tags=["tools"])


class ExecRequest(BaseModel):
    session_id: str
    command: str
    mode: str = "capture"  # capture or tty


class InstallRequest(BaseModel):
    session_id: str
    command: str


class LspRequest(BaseModel):
    session_id: str
    file_path: str
    line: int
    character: int
    new_name: str | None = None


class DocsRequest(BaseModel):
    url: str


class Context7DocsRequest(BaseModel):
    """Request for Context7 library documentation."""
    library: str
    topic: str | None = None
    max_tokens: int = 5000


class RepomapRequest(BaseModel):
    session_id: str


@router.post("/exec")
async def exec_tool(payload: ExecRequest) -> dict:
    container_id = await STATE.session_manager.get_session(payload.session_id)
    if not container_id:
        raise APIError(404, ErrorCodes.SESSION_NOT_FOUND, "Session not found", session_id=payload.session_id)
    
    decision = check_command(payload.command)
    log_command(payload.session_id, payload.command, decision)
    
    if decision == "DENY":
        raise APIError(
            400, ErrorCodes.POLICY_DENIED, 
            f"Command denied by policy: {payload.command}",
            details={"command": payload.command},
            session_id=payload.session_id
        )
    if decision == "ASK":
        raise APIError(
            400, ErrorCodes.POLICY_REQUIRES_APPROVAL,
            f"Requires approval: {payload.command}",
            details={"command": payload.command},
            session_id=payload.session_id
        )
        
    if payload.mode == "tty":
        run_id = new_id()
        await STATE.session_manager.register_command(run_id, payload.command)
        return {"run_id": run_id}
    stdout, stderr, code = await STATE.runtime.exec_capture_async(container_id, payload.command)
    log_command(payload.session_id, payload.command, decision, exit_code=code)
    return {"stdout": stdout, "stderr": stderr, "exit_code": code}


@router.post("/install")
async def install_tool(payload: InstallRequest) -> dict:
    container_id = await STATE.session_manager.get_session(payload.session_id)
    if not container_id:
        raise APIError(404, ErrorCodes.SESSION_NOT_FOUND, "Session not found", session_id=payload.session_id)
    output = await STATE.runtime.run_install_command_async(container_id, payload.command)
    return {"output": output}


@router.post("/repomap")
async def repomap(payload: RepomapRequest) -> dict:
    """RepoMap v2: Enhanced Tree-sitter symbol extraction."""
    container_id = await STATE.session_manager.get_session(payload.session_id)
    if not container_id:
        raise APIError(404, ErrorCodes.SESSION_NOT_FOUND, "Session not found", session_id=payload.session_id)
    
    repomap_data = await generate_repomap_v2(STATE.runtime, container_id)
    log_event(AuditEvent.AGENT_STEP, session_id=payload.session_id, data={"action": "repomap", "stats": repomap_data.get("cache_stats")})
    return {"repomap": repomap_data}


@router.post("/docs/fetch")
def docs_fetch(payload: DocsRequest) -> dict:
    # data = fetch_and_cache(payload.url)
    # return {"data": data}
    return {"data": "Documentation fetching temporarily disabled pending Context7 update."}


@router.post("/docs/context7")
async def docs_context7(payload: Context7DocsRequest) -> dict:
    """
    Fetch up-to-date library documentation via Context7.
    
    Context7 provides real-time, version-specific documentation
    to prevent LLM hallucinations when generating code.
    
    Example:
        POST /tools/docs/context7
        {"library": "fastapi", "topic": "routing"}
    """
    from app.tools.docs.context7_stub import get_context7_client
    
    client = get_context7_client()
    docs = await client.get_library_docs(
        library=payload.library,
        topic=payload.topic,
        max_tokens=payload.max_tokens,
    )
    
    return {
        "library": payload.library,
        "docs": [
            {
                "title": d.title,
                "content": d.content,
                "url": d.url,
                "version": d.version,
            }
            for d in docs
        ],
    }


@router.post("/lsp/rename")
async def lsp_rename(payload: LspRequest) -> dict:
    if payload.new_name is None:
        raise HTTPException(status_code=400, detail="new_name required")
    container_id = await STATE.session_manager.get_session(payload.session_id)
    if not container_id:
        raise HTTPException(status_code=404, detail="session not found")
    return await rename(
        STATE.runtime,
        container_id,
        payload.file_path,
        payload.line,
        payload.character,
        payload.new_name,
    )


@router.post("/lsp/refs")
async def lsp_refs(payload: LspRequest) -> dict:
    container_id = await STATE.session_manager.get_session(payload.session_id)
    if not container_id:
        raise HTTPException(status_code=404, detail="session not found")
    return await references(
        STATE.runtime,
        container_id,
        payload.file_path,
        payload.line,
        payload.character,
    )


@router.post("/lsp/defs")
async def lsp_defs(payload: LspRequest) -> dict:
    container_id = await STATE.session_manager.get_session(payload.session_id)
    if not container_id:
        raise HTTPException(status_code=404, detail="session not found")
    return await definitions(
        STATE.runtime,
        container_id,
        payload.file_path,
        payload.line,
        payload.character,
    )


# ==================== OWNSTACK INNOVATIONS ====================


class HealRequest(BaseModel):
    """Request to self-heal a failing command."""
    session_id: str
    command: str
    max_attempts: int = 5


@router.post("/heal")
async def self_heal(payload: HealRequest) -> dict:
    """
    OwnStack Innovation: Self-Healing CI
    
    Automatically detects failures, generates fixes, applies them
    in the sandbox, and validates - all locally, no cloud needed.
    """
    from app.agent.healer import get_healer
    
    container_id = await STATE.session_manager.get_session(payload.session_id)
    if not container_id:
        raise APIError(404, ErrorCodes.SESSION_NOT_FOUND, "Session not found")
    
    healer = get_healer(STATE.runtime)
    session = await healer.heal(container_id, payload.command, payload.max_attempts)
    
    log_event(AuditEvent.COMMAND_EXEC, session_id=payload.session_id, data={
        "action": "self_heal",
        "healed": session.healed,
        "attempts": session.total_attempts,
    })
    
    return healer.get_healing_summary(session)


class MultiversRequest(BaseModel):
    """Request to run parallel variant tests."""
    session_id: str
    command: str
    variants: Dict[str, Dict[str, Any]]


@router.post("/multivers")
async def multivers_run(payload: MultiversRequest) -> dict:
    """
    OwnStack Innovation: Multivers Infra - A/B Testing at System Level
    
    Fork a session into multiple variants (different Python versions,
    library versions, configs) and run the same command in parallel.
    Returns comparison of all results.
    """
    from app.runtime.multivers import get_multivers
    
    container_id = await STATE.session_manager.get_session(payload.session_id)
    if not container_id:
        raise APIError(404, ErrorCodes.SESSION_NOT_FOUND, "Session not found")
    
    multivers = get_multivers(STATE.runtime)
    run = await multivers.fork_and_run(
        base_session_id=payload.session_id,
        command=payload.command,
        variants=payload.variants,
    )
    
    log_event(AuditEvent.COMMAND_EXEC, session_id=payload.session_id, data={
        "action": "multivers",
        "variants": list(payload.variants.keys()),
        "completed": run.completed,
    })
    
    return run.get_comparison()


@router.get("/sense/health")
async def sense_health() -> dict:
    """
    OwnStack Innovation: InfraSense - Infrastructure Awareness
    
    Returns real-time health stats of the agent ecosystem.
    - Active containers
    - Total resources used
    - Stressed agents
    """
    from app.runtime.sense import get_sense
    
    sense = get_sense(STATE.runtime)
    health = await sense.get_system_health()
    
    return {
        "status": "healthy" if not health.warnings else "warning",
        "active_agents": health.active_containers,
        "resources": {
            "total_cpu_percent": health.total_cpu_percent,
            "total_memory_mb": health.total_memory_mb,
        },
        "warnings": health.warnings,
    }


# ============================================================
# AUTONOMOUS AGENT - The Core Innovation
# ============================================================

class AgentRequest(BaseModel):
    """Request to run the autonomous engineering agent."""
    session_id: str
    instructions: str
    provider: str = "openai"
    model: str = "gpt-4o"
    api_key: str | None = None
    verify_command: str | None = None
    max_steps: int = 10


@router.post("/agent/run")
async def run_agent(payload: AgentRequest) -> dict:
    """
    OwnStack Core: Autonomous Engineering Agent
    
    Runs an AI agent that can:
    - Read and write files
    - Execute commands
    - Self-correct on errors
    - Verify results
    
    Supports: OpenAI, Anthropic, Ollama, any OpenAI-compatible API.
    """
    from app.agent.core import create_agent, AgentConfig
    
    container_id = await STATE.session_manager.get_session(payload.session_id)
    if not container_id:
        raise APIError(404, ErrorCodes.SESSION_NOT_FOUND, "Session not found")
    
    agent = create_agent(
        session_id=payload.session_id,
        runtime=STATE.runtime,
        container_id=container_id,
        provider=payload.provider,
        model=payload.model,
        api_key=payload.api_key,
        verify_command=payload.verify_command,
        max_steps=payload.max_steps or 20,
    )
    
    try:
        events = []
        async for event in agent.run_stream(payload.instructions):
            events.append(event.model_dump())
            
            # Log significant events
            if event.event in ("tool_call", "complete", "error"):
                try:
                    log_event(AuditEvent.AGENT_ACTION, session_id=payload.session_id, data={
                        "event": event.event,
                        "data": event.data,
                    })
                except Exception as e:
                    logger.error(f"log_event failed: {e}")
        
        # Create snapshot after agent run
        tm = get_time_machine(STATE.runtime)
        snapshot = await tm.snapshot(container_id, f"Agent: {payload.instructions[:30]}...")
        
        # Extract summary for extension compatibility
        error_event = next((e["data"] for e in events if e["event"] == "error"), None)
        complete_event = next((e["data"] for e in events if e["event"] == "complete"), {})
        
        steps = complete_event.get("steps", len([e for e in events if e["event"] == "step"]))
        
        if error_event:
            result_text = f"❌ Error: {error_event.get('error')}"
        else:
            result_text = complete_event.get("response") or "Agent finished without final response."

        return {
            "session_id": payload.session_id,
            "events": events,
            "completed": not error_event and any(e["event"] == "complete" for e in events),
            "steps": steps,
            "result": result_text,
            "snapshot": {
                "id": snapshot.id,
                "message": snapshot.message,
                "timestamp": snapshot.timestamp
            }
        }
    except Exception as e:
        import traceback
        logger.error(f"agent_run CRASH: {e}")
        logger.error(traceback.format_exc())
        raise APIError(500, ErrorCodes.INTERNAL_ERROR, str(e))


@router.get("/providers")
async def list_providers() -> dict:
    """List available LLM providers."""
    from app.agent.providers import list_providers as get_providers
    
    return {
        "providers": get_providers(),
        "default": "openai",
    }


# ============================================================
# SECURE BROWSER - Computer Use
# ============================================================

class BrowseRequest(BaseModel):
    session_id: str
    url: str


@router.post("/browser/browse")
async def secure_browse(payload: BrowseRequest) -> dict:
    """
    OwnStack Innovation: Secure Browser - Isolated 'Computer Use'.
    
    Allows the agent to browse documentation and resources SAFELY.
    - Domain Allow-list enforced
    - No cookies/storage persistence
    - Ad/Tracker blocking
    """
    from app.tools.browser import get_secure_browser
    
    # Check session
    container_id = await STATE.session_manager.get_session(payload.session_id)
    if not container_id:
        raise APIError(404, ErrorCodes.SESSION_NOT_FOUND, "Session not found")
        
    try:
        browser = get_secure_browser()
        result = await browser.browse(payload.url)
        
        log_event(AuditEvent.AGENT_ACTION, session_id=payload.session_id, data={
            "action": "browse",
            "url": payload.url,
            "success": "error" not in result
        })
        
        return result
    except Exception as e:
        raise APIError(500, ErrorCodes.INTERNAL_ERROR, str(e))


# ============================================================
# TIME MACHINE - Safe Rollback
# ============================================================

class SnapshotRequest(BaseModel):
    session_id: str
    message: str

class RollbackRequest(BaseModel):
    session_id: str
    snapshot_id: str


@router.post("/time/snapshot")
async def create_snapshot(payload: SnapshotRequest) -> dict:
    """Create a code checkpoint."""
    from app.tools.git_time import get_time_machine
    
    container_id = await STATE.session_manager.get_session(payload.session_id)
    if not container_id:
        raise APIError(404, ErrorCodes.SESSION_NOT_FOUND, "Session not found")
        
    tm = get_time_machine(STATE.runtime)
    snapshot = await tm.snapshot(container_id, payload.message)
    
    return {
        "id": snapshot.id,
        "message": snapshot.message,
        "timestamp": snapshot.timestamp
    }


@router.post("/time/rollback")
async def rollback_snapshot(payload: RollbackRequest) -> dict:
    """Rollback code to a previous checkpoint."""
    from app.tools.git_time import get_time_machine
    
    container_id = await STATE.session_manager.get_session(payload.session_id)
    if not container_id:
        raise APIError(404, ErrorCodes.SESSION_NOT_FOUND, "Session not found")
        
    tm = get_time_machine(STATE.runtime)
    success = await tm.rollback(container_id, payload.snapshot_id)
    
    if success:
        log_event(AuditEvent.AGENT_ACTION, session_id=payload.session_id, data={
            "action": "rollback",
            "snapshot_id": payload.snapshot_id
        })
        
    return {"success": success}


# ============================================================
# AGENT STREAMING
# ============================================================

@router.post("/agent/stream")
async def run_agent_stream(payload: AgentRequest):
    """
    Stream the agent's execution event by event (SSE).
    """
    from app.agent.core import create_agent
    
    container_id = await STATE.session_manager.get_session(payload.session_id)
    if not container_id:
        raise APIError(404, ErrorCodes.SESSION_NOT_FOUND, "Session not found")
    
    # Create agent instance
    agent = create_agent(
        session_id=payload.session_id,
        runtime=STATE.runtime,
        container_id=container_id,
        provider=payload.provider,
        model=payload.model,
        api_key=payload.api_key,
        verify_command=payload.verify_command,
        max_steps=payload.max_steps or 20,
    )

    async def event_generator():
        try:
            async for event in agent.run_stream(payload.instructions):
                # Send data as JSON event
                data = json.dumps(event.model_dump())
                yield f"data: {data}\n\n"
                
                # Log significant events asynchronously
                if event.event in ("tool_call", "complete", "error"):
                    try:
                        log_event(AuditEvent.AGENT_ACTION, session_id=payload.session_id, data={
                            "event": event.event,
                            "data": event.data,
                        })
                    except:
                        pass
                        
            # Force a final sync if needed, though 'complete' event handles it
            
        except Exception as e:
            error_data = json.dumps({"event": "error", "data": {"error": str(e)}})
            yield f"data: {error_data}\n\n"

    return StreamingResponse(event_generator(), media_type="text/event-stream")
