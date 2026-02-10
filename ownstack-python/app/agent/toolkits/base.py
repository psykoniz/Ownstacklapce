"""Base classes for the Toolkit system (SOTA Architecture)."""
from __future__ import annotations
from abc import ABC, abstractmethod
from typing import Dict, Any, List, Callable, Awaitable
from pydantic import BaseModel

from app.agent.providers import ToolDefinition, ToolCall

class Toolkit(ABC):
    """Base class for all agent toolkits."""
    
    def __init__(self, runtime: Any, container_id: str):
        self.runtime = runtime
        self.container_id = container_id

    @abstractmethod
    def get_definitions(self) -> List[ToolDefinition]:
        """Return the list of tool definitions provided by this toolkit."""
        pass

    @abstractmethod
    def get_handlers(self) -> Dict[str, Callable[[ToolCall], Awaitable[Dict[str, Any]]]]:
        """Return a mapping of tool names to their async handlers."""
        pass
