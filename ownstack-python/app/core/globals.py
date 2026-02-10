"""Global application state and configuration."""
from __future__ import annotations
import os
from dataclasses import dataclass, field
from typing import Dict, Optional, Any
import asyncio

from app.runtime.manager import RuntimeManager
from app.missions.manager import MissionManager
from app.agent.providers.base import get_provider, ProviderConfig


@dataclass
class SessionManager:
    _sessions: Dict[str, str] = field(default_factory=dict)
    _run_commands: Dict[str, str] = field(default_factory=dict)
    _lock: asyncio.Lock = field(default_factory=asyncio.Lock)

    async def get_session(self, session_id: str) -> Optional[str]:
        async with self._lock:
            return self._sessions.get(session_id)

    async def list_sessions(self) -> list[dict]:
        async with self._lock:
            return [{"session_id": k, "container_id": v} for k, v in self._sessions.items()]

    async def register_session(self, session_id: str, container_id: str):
        async with self._lock:
            self._sessions[session_id] = container_id

    async def remove_session(self, session_id: str) -> Optional[str]:
        async with self._lock:
            return self._sessions.pop(session_id, None)

    async def get_command(self, run_id: str) -> Optional[str]:
        async with self._lock:
            return self._run_commands.get(run_id)

    async def register_command(self, run_id: str, command: str):
        async with self._lock:
            self._run_commands[run_id] = command

    async def pop_command(self, run_id: str) -> Optional[str]:
        async with self._lock:
            return self._run_commands.pop(run_id, None)

    async def get_active_count(self) -> int:
        async with self._lock:
            return len(self._sessions)


@dataclass
class AppState:
    runtime: RuntimeManager
    session_manager: SessionManager = field(default_factory=SessionManager)
    mission_manager: Optional[MissionManager] = None
    agent_provider: Optional[Any] = None # Added for mission planning


@dataclass(frozen=True)
class Settings:
    docker_image: str = os.getenv("IDE_AGENT_IMAGE", "ide-agent-env:v1")
    workspace_root: str = os.getenv("IDE_AGENT_WORKSPACE", os.getcwd())
    cache_volume: str = os.getenv("IDE_AGENT_CACHE_VOLUME", "ide-agent-cache")
    github_webhook_secret: Optional[str] = os.getenv("GITHUB_WEBHOOK_SECRET")
    workspace_host_path: Optional[str] = os.getenv("IDE_AGENT_WORKSPACE_HOST_PATH")
    webhook_rate_limit: int = int(os.getenv("WEBHOOK_RATE_LIMIT", "30"))
    webhook_rate_window_s: int = int(os.getenv("WEBHOOK_RATE_WINDOW_S", "60"))


SETTINGS = Settings()
STATE = AppState(runtime=RuntimeManager(SETTINGS))
# Initialize mission manager with a writable path (we mount .ownstack at /app/.ownstack)
STATE.mission_manager = MissionManager("/app")


# SOTA Phase 75: Security Configuration (Dual Auth)
@dataclass(frozen=True)
class SecurityConfig:
    SECRET_KEY: str = os.getenv("OWNSTACK_SECRET_KEY", "os-dev-secret-change-me")
    ALGORITHM: str = "HS256"
    ACCESS_TOKEN_EXPIRE_MINUTES: int = 30
    REFRESH_TOKEN_EXPIRE_DAYS: int = 7

SECURITY_CONFIG = SecurityConfig()

# Initialize agent provider with auto-detection (P1 fix)
# Priority: OPENROUTER > ANTHROPIC > OPENAI
def _init_agent_provider():
    """Auto-detect and initialize the agent provider."""
    from app.core.preflight import detect_provider_from_key
    
    # Check keys in priority order
    keys = [
        ("OPENROUTER_API_KEY", "openrouter"),
        ("ANTHROPIC_API_KEY", "anthropic"),
        ("OPENAI_API_KEY", "openai"),
    ]
    
    for env_var, _ in keys:
        api_key = os.getenv(env_var)
        if api_key:
            provider, model = detect_provider_from_key(api_key)
            config = ProviderConfig(
                provider=provider,
                model=model,
                api_key=api_key
            )
            try:
                return get_provider(config)
            except Exception:
                continue
    
    return None

STATE.agent_provider = _init_agent_provider()
