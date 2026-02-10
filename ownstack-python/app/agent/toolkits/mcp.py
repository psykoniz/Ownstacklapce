"""Toolkit for MCP (Model Context Protocol) external tools."""
from typing import Dict, Any, List, Callable, Awaitable

from app.agent.toolkits.base import Toolkit
from app.agent.providers import ToolDefinition, ToolCall

class MCPToolkit(Toolkit):
    """Toolkit for dynamic MCP tools."""
    
    def __init__(self, runtime: Any, container_id: str, mcp_clients: List[Any]):
        super().__init__(runtime, container_id)
        self.mcp_clients = mcp_clients

    def get_definitions(self) -> List[ToolDefinition]:
        # MCP definitions are fetched dynamically in _get_tools of BaseAgent
        return []

    def get_handlers(self) -> Dict[str, Callable[[ToolCall], Awaitable[Dict[str, Any]]]]:
        # We don't return static handlers because tool names are dynamic (mcp_*)
        # Instead, the agent core loop or a special catch-all in the registry handles it.
        # For now, we'll implement a catch-all method.
        return {}

    async def handle_mcp_call(self, call: ToolCall) -> Dict[str, Any]:
        tool_name = call.name[4:]  # Remove 'mcp_' prefix
        for client in self.mcp_clients:
            # Note: This is simplified. In a real scenario, we'd know which client owns which tool.
            result = await client.call_tool(tool_name, call.arguments)
            if result:
                return {"content": str(result.get("content", ""))}
        return {"content": f"MCP tool {tool_name} not found", "error": "not_found"}
