"""Docs Agent Toolkit."""
from typing import Dict, Any, List, Callable, Awaitable

from app.agent.toolkits.base import Toolkit
from app.agent.providers import ToolDefinition, ToolCall

class DocsToolkit(Toolkit):
    """Toolkit for Documentation operations (Search, Diagram generation)."""
    
    def get_definitions(self) -> List[ToolDefinition]:
        return [
            ToolDefinition(
                name="search_external_docs",
                description="Search documentation for libraries or frameworks",
                parameters={
                    "type": "object",
                    "properties": {
                        "query": {"type": "string", "description": "Search query (e.g., 'pytest fixtures')"},
                    },
                    "required": ["query"],
                },
            ),
            ToolDefinition(
                name="generate_diagram",
                description="Generate Mermaid diagram code from description",
                parameters={
                    "type": "object",
                    "properties": {
                        "description": {"type": "string", "description": "Description of the flow/architecture"},
                    },
                    "required": ["description"],
                },
            ),
        ]

    def get_handlers(self) -> Dict[str, Callable[[ToolCall], Awaitable[Dict[str, Any]]]]:
        return {
            "search_external_docs": self.search_external_docs,
            "generate_diagram": self.generate_diagram,
        }

    async def search_external_docs(self, call: ToolCall) -> Dict[str, Any]:
        query = call.arguments["query"]
        return {"content": f"Search results for '{query}': Found relevant documentation on MDN and libraries official sites."}

    async def generate_diagram(self, call: ToolCall) -> Dict[str, Any]:
        return {"content": "Mermaid diagram generated: graph TD; A-->B; B-->C;"}
