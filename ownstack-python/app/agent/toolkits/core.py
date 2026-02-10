"""Core toolkit for standard system operations (Files, Commands)."""
import json
import logging
from typing import Dict, Any, List, Callable, Awaitable

from app.agent.toolkits.base import Toolkit
from app.agent.providers import ToolDefinition, ToolCall
from app.utils.path_safety import validate_read_path, validate_write_path

class CoreToolkit(Toolkit):
    """Toolkit for reading/writing files and executing commands."""
    
    def get_definitions(self) -> List[ToolDefinition]:
        return [
            ToolDefinition(
                name="read_file",
                description="Lire le contenu d'un fichier (UTF-8)",
                parameters={
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Chemin absolu du fichier"},
                    },
                    "required": ["path"],
                },
            ),
            ToolDefinition(
                name="write_file",
                description="Écrire (écraser) le contenu d'un fichier",
                parameters={
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Chemin absolu du fichier"},
                        "content": {"type": "string", "description": "Contenu à écrire"},
                    },
                    "required": ["path", "content"],
                },
            ),
            ToolDefinition(
                name="execute_command",
                description="Exécuter une commande shell dans la sandbox",
                parameters={
                    "type": "object",
                    "properties": {
                        "command": {"type": "string", "description": "La commande à exécuter"},
                    },
                    "required": ["command"],
                },
            ),
            ToolDefinition(
                name="apply_patch",
                description="Appliquer un patch diff unifié pour modifier des fichiers",
                parameters={
                    "type": "object",
                    "properties": {
                        "unified_diff": {"type": "string", "description": "Contenu du patch diff"},
                    },
                    "required": ["unified_diff"],
                },
            ),
            ToolDefinition(
                name="search_dir",
                description="Rechercher une chaîne de caractères dans un répertoire de façon récursive",
                parameters={
                    "type": "object",
                    "properties": {
                        "query": {"type": "string", "description": "Chaîne à rechercher"},
                        "path": {"type": "string", "description": "Répertoire de recherche (défaut: .)"},
                    },
                    "required": ["query"],
                },
            ),
            ToolDefinition(
                name="execute_network_command",
                description="Exécuter une commande avec accès INTERNET (installs, tests API)",
                parameters={
                    "type": "object",
                    "properties": {
                        "cmd": {"type": "string", "description": "Commande à exécuter"},
                    },
                    "required": ["cmd"],
                },
            ),
        ]

    def get_handlers(self) -> Dict[str, Callable[[ToolCall], Awaitable[Dict[str, Any]]]]:
        return {
            "read_file": self.read_file,
            "write_file": self.write_file,
            "execute_command": self.execute_command,
            "apply_patch": self.apply_patch,
            "search_dir": self.search_dir,
            "execute_network_command": self.execute_network_command,
        }

    async def read_file(self, call: ToolCall) -> Dict[str, Any]:
        path = call.arguments["path"]
        validate_read_path(self.runtime.settings.workspace_root, path)
        stdout, stderr, exit_code = await self.runtime.exec_capture_async(
            self.container_id, f"cat '{path}'"
        )
        if exit_code != 0:
            return {"content": f"Error: {stderr}", "exit_code": exit_code}
        return {"content": stdout}

    async def write_file(self, call: ToolCall) -> Dict[str, Any]:
        path = call.arguments["path"]
        content = call.arguments["content"]
        validate_write_path(self.runtime.settings.workspace_root, path)
        
        # SOTA: Improved writing via temp file + mv to avoid pipe truncation for large files
        tmp_path = f"{path}.tmp"
        # Using a safer way to pipe content
        await self.runtime.exec_capture_async(self.container_id, f"cat << 'EOF' > '{tmp_path}'\n{content}\nEOF")
        stdout, stderr, exit_code = await self.runtime.exec_capture_async(
            self.container_id, f"mv '{tmp_path}' '{path}'"
        )
        if exit_code != 0:
            return {"content": f"Error: {stderr}", "exit_code": exit_code}
        return {"content": f"File written successfully to {path}"}

    async def execute_command(self, call: ToolCall) -> Dict[str, Any]:
        command = call.arguments["command"]
        stdout, stderr, exit_code = await self.runtime.exec_capture_async(self.container_id, command)
        return {
            "content": f"[STDOUT]\n{stdout}\n[STDERR]\n{stderr}",
            "exit_code": exit_code
        }

    async def apply_patch(self, call: ToolCall) -> Dict[str, Any]:
        import uuid
        diff = call.arguments["unified_diff"]
        patch_path = f"/tmp/patch_{uuid.uuid4().hex}.diff"
        await self.runtime.write_file_async(self.container_id, patch_path, diff)
        stdout, stderr, code = await self.runtime.exec_capture_async(
            self.container_id,
            f"git apply --unsafe-paths {patch_path}"
        )
        if code != 0:
            return {"content": f"{stdout}\n{stderr}", "error": "patch_failed"}
        return {"content": "Patch applied successfully"}

    async def search_dir(self, call: ToolCall) -> Dict[str, Any]:
        query = call.arguments["query"]
        path = call.arguments.get("path", ".")
        cmd = f"grep -rl \"{query}\" {path} | head -n 20"
        stdout, stderr, code = await self.runtime.exec_capture_async(self.container_id, cmd)
        if not stdout.strip():
            return {"content": f"No matches found for '{query}' in {path}"}
        return {"content": f"Matches found in:\n{stdout}"}

    async def execute_network_command(self, call: ToolCall) -> Dict[str, Any]:
        # Note: check_command logic is handled in the agent core loop for consolidated policy enforcement
        cmd = call.arguments["cmd"]
        output = await self.runtime.run_networked_command_async(self.container_id, cmd)
        return {"content": output}
