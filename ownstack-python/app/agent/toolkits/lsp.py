"""LSP toolkit for advanced code navigation and refactoring."""
import json
from typing import Dict, Any, List, Callable, Awaitable

from app.agent.toolkits.base import Toolkit
from app.agent.providers import ToolDefinition, ToolCall
from app.tools.lsp import client as lsp
from app.utils.path_safety import validate_read_path, validate_write_path

class LSPToolkit(Toolkit):
    """Toolkit for Language Server Protocol operations."""
    
    def get_definitions(self) -> List[ToolDefinition]:
        return [
            ToolDefinition(
                name="lsp_rename",
                description="Rename a symbol across the project using LSP. This is safer than bulk text replacement.",
                parameters={
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Absolute path to the file containing the symbol"},
                        "line": {"type": "integer", "description": "0-based line number of the symbol"},
                        "character": {"type": "integer", "description": "0-based character offset of the symbol"},
                        "new_name": {"type": "string", "description": "The new name for the symbol"},
                    },
                    "required": ["path", "line", "character", "new_name"],
                },
            ),
            ToolDefinition(
                name="lsp_definitions",
                description="Find the definition of a symbol using LSP.",
                parameters={
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Absolute path to the file"},
                        "line": {"type": "integer", "description": "0-based line number"},
                        "character": {"type": "integer", "description": "0-based character offset"},
                    },
                    "required": ["path", "line", "character"],
                },
            ),
            ToolDefinition(
                name="lsp_references",
                description="Find all references of a symbol using LSP.",
                parameters={
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Absolute path to the file"},
                        "line": {"type": "integer", "description": "0-based line number"},
                        "character": {"type": "integer", "description": "0-based character offset"},
                    },
                    "required": ["path", "line", "character"],
                },
            ),
        ]

    def get_handlers(self) -> Dict[str, Callable[[ToolCall], Awaitable[Dict[str, Any]]]]:
        return {
            "lsp_rename": self.lsp_rename,
            "lsp_definitions": self.lsp_definitions,
            "lsp_references": self.lsp_references,
        }

    async def lsp_rename(self, call: ToolCall) -> Dict[str, Any]:
        path = call.arguments["path"]
        line = call.arguments["line"]
        character = call.arguments["character"]
        new_name = call.arguments["new_name"]
        
        validate_write_path(self.runtime.settings.workspace_root, path)
        result = await lsp.rename(self.runtime, self.container_id, path, line, character, new_name)
        return {"content": f"LSP Rename applied. Updated files: {result.get('updated', [])}"}

    async def lsp_definitions(self, call: ToolCall) -> Dict[str, Any]:
        path = call.arguments["path"]
        line = call.arguments["line"]
        character = call.arguments["character"]
        
        validate_read_path(self.runtime.settings.workspace_root, path)
        result = await lsp.definitions(self.runtime, self.container_id, path, line, character)
        return {"content": json.dumps(result.get("definitions", []), indent=2)}

    async def lsp_references(self, call: ToolCall) -> Dict[str, Any]:
        path = call.arguments["path"]
        line = call.arguments["line"]
        character = call.arguments["character"]
        
        validate_read_path(self.runtime.settings.workspace_root, path)
        result = await lsp.references(self.runtime, self.container_id, path, line, character)
        return {"content": json.dumps(result.get("references", []), indent=2)}
