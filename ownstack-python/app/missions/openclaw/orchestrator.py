from ..worker_bridge import ClaudeCodeBridge, NativeAgentBridge
from .planner import OpenClawPlanner
from app.core.globals import STATE
import logging
from app.missions.models import MissionStatus

logger = logging.getLogger(__name__)

class OpenClawOrchestrator:
    """The 'Brain' of the autopilot. Orchestrates the Mission loop."""
    
    def __init__(self, mission_manager):
        self.mission_manager = mission_manager
        self._provider = STATE.agent_provider # Assuming global provider for planning
        self.planner = OpenClawPlanner(self._provider)

    async def run_mission(self, mission_id: str):
        """Execute the full lifecycle of a mission."""
        mission = self.mission_manager.get_mission(mission_id)
        if not mission:
            logger.error(f"Mission {mission_id} not found for orchestration")
            return

        try:
            # 1. Planning Phase (Copy concept from OpenClaw)
            await self.mission_manager.update_status(mission_id, MissionStatus.PLANNING, "Analyzing task and generating plan...")
            
            # Pass compiled spec if available (Unification Phase 72)
            spec_dict = mission.spec.model_dump() if mission.spec else None
            plan = await self.planner.generate_plan(mission.description, spec=spec_dict)
            mission.metadata["plan"] = plan.dict()
            await self.mission_manager.add_log(mission_id, f"Plan generated: {plan.rationale}", {"plan": plan.dict()})
            
            # 2. Start Execution
            await self.mission_manager.update_status(mission_id, MissionStatus.RUNNING, "Executing plan steps...")
            
            # Select Worker Bridge
            container_id = mission.metadata.get("container_id")
            session_id = mission.metadata.get("session_id")
            
            if mission.worker_type == "ownstack_agent" and container_id and session_id:
                worker = NativeAgentBridge(
                    session_id=session_id, 
                    runtime=STATE.runtime, 
                    container_id=container_id
                )
            else:
                # Fallback to Claude CLI if not ownstack_agent or missing session
                worker = ClaudeCodeBridge()
                if mission.worker_type == "ownstack_agent":
                    await self.mission_manager.add_log(mission_id, "[WARNING] Missing session/container for NativeAgent. Falling back to Claude CLI.")

            # For each step in the plan, execute using the worker
            for step in plan.steps:
                await self.mission_manager.add_log(mission_id, f"Step {step.id}: {step.summary}")
                
                # Execute worker task for this specific step
                async for chunk in worker.run_task(f"Context: {plan.rationale}\nTask: {step.summary}", mission.project_path):
                    await self.mission_manager.add_log(mission_id, chunk, {"type": "terminal_chunk", "step_id": step.id})
                
                # Intermediate Verification (Inspired by OpenClaw/Judge)
                # In Phase 43, we'll do a simple 'is still compiling' check
            
            # 3. Final Verification (Judge Phase)
            await self.mission_manager.update_status(mission_id, MissionStatus.VERIFYING, "Running final verification oracles...")
            
            # Run oracles - for now, we just run pytest
            # We use the state runtime to run verification in the same way OwnStack does normally
            # (Dogfooding principle: use CowStack tools on OwnStack code)
            container_id = mission.metadata.get("container_id")
            if not container_id:
                # If no container specified, launch a temporary one or use host (DANGEROUS)
                # For Dogfood, we assume a container is attached to the mission
                await self.mission_manager.add_log(mission_id, "[WARNING] No sandbox container assigned. Verification skipped for safety.")
                await self.mission_manager.update_status(mission_id, MissionStatus.NEEDS_REVIEW, "Execution complete, but verification skipped.")
                return

            # Run verify command in container
            verify_cmd = "pytest" # Or whatever is defined in the mission
            await self.mission_manager.add_log(mission_id, f"Running verification in container {container_id}: {verify_cmd}")
            
            output = ""
            async for chunk in STATE.runtime.exec_stream_tty_async(container_id, verify_cmd):
                output += chunk.decode()
            
            if "failed" in output.lower() or "error" in output.lower():
                await self.mission_manager.add_log(mission_id, f"Verification FAILED:\n{output}")
                await self.mission_manager.update_status(mission_id, MissionStatus.FAILED, "Verification failed. Check logs.")
            else:
                await self.mission_manager.add_log(mission_id, f"Verification PASSED:\n{output}")
                await self.mission_manager.update_status(mission_id, MissionStatus.COMPLETED, "Mission accomplished successfully.")

        except Exception as e:
            logger.exception(f"Crash in OpenClawOrchestrator for mission {mission_id}")
            await self.mission_manager.add_log(mission_id, f"CRITICAL ERROR: {str(e)}")
            await self.mission_manager.update_status(mission_id, MissionStatus.FAILED, f"Orchestrator crash: {str(e)}")
