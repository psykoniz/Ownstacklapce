import os
import json
import anyio
import logging
from typing import List, Dict, Optional, Any
from pathlib import Path
from .models import Mission, MissionStatus

logger = logging.getLogger(__name__)

class MissionManager:
    """Manages autonomous missions and their persistence."""
    
    def __init__(self, workspace_root: str):
        self.workspace_root = Path(workspace_root)
        self.missions_dir = self.workspace_root / ".ownstack" / "missions"
        self.missions_dir.mkdir(parents=True, exist_ok=True)
        self._cache: Dict[str, Mission] = {}
        self._subscribers: List[Any] = []
        self._load_all()

    def subscribe(self, callback):
        """Register a callback for mission events."""
        self._subscribers.append(callback)

    def unsubscribe(self, callback):
        """Unregister a callback."""
        if callback in self._subscribers:
            self._subscribers.remove(callback)

    async def _notify(self, event_type: str, data: Any):
        """Notify all subscribers of an event."""
        import asyncio
        for cb in self._subscribers:
            try:
                if asyncio.iscoroutinefunction(cb):
                    await cb(event_type, data)
                else:
                    cb(event_type, data)
            except Exception as e:
                logger.error(f"Broadcasting error: {e}")

    def _load_all(self):
        """
        Pre-load existing missions from disk.
        SOTA Phase 76: Robust loading with corruption handling.
        """
        # Cleanup left-over tmp files from previous crashes
        for tmp_file in self.missions_dir.glob("*.tmp"):
            try:
                tmp_file.unlink()
            except Exception:
                pass

        for mission_file in self.missions_dir.glob("*.json"):
            try:
                with open(mission_file, "r") as f:
                    data = json.load(f)
                    mission = Mission.model_validate(data)
                    self._cache[mission.id] = mission
            except json.JSONDecodeError:
                logger.error(f"CORRUPTED MISSION FILE: {mission_file} - Moving to .corrupted")
                # Move to .corrupted to prevent loop crash
                corrupted_dir = self.missions_dir / ".corrupted"
                corrupted_dir.mkdir(exist_ok=True)
                try:
                    import shutil
                    shutil.move(str(mission_file), str(corrupted_dir / mission_file.name))
                except Exception:
                    pass
            except Exception as e:
                logger.error(f"Failed to load mission {mission_file}: {e}")

    async def save_mission(self, mission: Mission):
        """
        SOTA Phase 76: Persist mission state to disk with atomic write.
        Prevents file corruption if process crashes during write.
        """
        self._cache[mission.id] = mission
        file_path = self.missions_dir / f"{mission.id}.json"
        
        # Use sync write in thread to avoid blocking
        def _atomic_write():
            # 1. Write to temp file
            temp_path = file_path.with_suffix(".tmp")
            try:
                with open(temp_path, "w") as f:
                    f.write(mission.model_dump_json(indent=2))
                # 2. Rename (Atomic on POSIX, usually safe on Windows)
                temp_path.replace(file_path)
            except Exception as e:
                logger.error(f"Failed to save mission {mission.id}: {e}")
                if temp_path.exists():
                    temp_path.unlink()

        await anyio.to_thread.run_sync(_atomic_write)

    async def create_checkpoint(self, mission_id: str, description: str) -> Optional[str]:
        """SOTA Phase 76: Create a checkpoint of the current mission state."""
        mission = self.get_mission(mission_id)
        if not mission:
            return None
            
        from .models import MissionCheckpoint
        checkpoint = MissionCheckpoint(
            description=description,
            mission_status=mission.status,
            events_count=len(mission.events),
            metadata_snapshot=mission.metadata.copy()
        )
        
        mission.checkpoints.append(checkpoint)
        mission.add_event("checkpoint", f"Created checkpoint: {description}", {"checkpoint_id": checkpoint.id})
        await self.save_mission(mission)
        return checkpoint.id

    async def archive_mission(self, mission_id: str):
        """Move mission to archive folder."""
        mission = self.get_mission(mission_id)
        if not mission: return
        
        archive_dir = self.missions_dir / "archive"
        archive_dir.mkdir(exist_ok=True)
        
        src = self.missions_dir / f"{mission_id}.json"
        dst = archive_dir / f"{mission_id}.json"
        
        if src.exists():
            import shutil
            await anyio.to_thread.run_sync(shutil.move, str(src), str(dst))
            
        # Update cache (remove) or mark as archived?
        # For now, remove from active cache
        self._cache.pop(mission_id, None)

    async def create_mission(self, title: str, description: str, worker_type: str = "claude_code") -> Mission:
        """Create and persist a new mission."""
        mission = Mission(
            title=title, 
            description=description, 
            worker_type=worker_type,
            project_path=str(self.workspace_root)
        )
        mission.add_event("status_change", f"Mission created: {title}", {"status": mission.status})
        await self.save_mission(mission)
        return mission

    def get_mission(self, mission_id: str) -> Optional[Mission]:
        """Retrieve a mission by ID."""
        return self._cache.get(mission_id)

    async def list_missions(self) -> List[Mission]:
        """List all known missions."""
        return list(self._cache.values())

    async def update_status(self, mission_id: str, status: MissionStatus, message: Optional[str] = None):
        """Update mission status and persist."""
        mission = self.get_mission(mission_id)
        if mission:
            mission.status = status
            mission.add_event("status_change", message or f"Status changed to {status}", {"status": status})
            await self.save_mission(mission)
            await self._notify("status_change", {"mission_id": mission_id, "status": status, "message": message})
            
    async def add_log(self, mission_id: str, message: str, data: Optional[Dict[str, Any]] = None):
        """Add a log event to a mission."""
        mission = self.get_mission(mission_id)
        if mission:
            mission.add_event("log", message, data)
            await self.save_mission(mission)
            await self._notify("log", {"mission_id": mission_id, "message": message, "data": data})
