import os
import uuid
import time
import asyncio
import hashlib
import logging
from typing import Any, Dict, List, Optional
from app.tools.lsp.edit import apply_workspace_edit
from app.tools.lsp.framing import decode_messages, sequence
from app.tools.lsp.hub import get_hub

logger = logging.getLogger(__name__)

LSP_MODE = os.getenv("LSP_MODE", "stateless")  # Options: stateless, persistent


class LSPCache:
    """
    SOTA Phase 78: In-memory cache for LSP responses.
    Caches definition and references lookups keyed by (file, line, char).
    """
    
    def __init__(self, ttl_seconds: int = 300):
        self._cache: Dict[str, tuple] = {}  # key -> (result, timestamp)
        self._ttl = ttl_seconds
    
    def _make_key(self, method: str, file_path: str, line: int, char: int) -> str:
        return hashlib.md5(f"{method}:{file_path}:{line}:{char}".encode()).hexdigest()
    
    def get(self, method: str, file_path: str, line: int, char: int) -> Optional[Dict]:
        key = self._make_key(method, file_path, line, char)
        if key in self._cache:
            result, ts = self._cache[key]
            if time.time() - ts < self._ttl:
                logger.info(f"[LSPCache] HIT for {method} at {file_path}:{line}")
                return result
            else:
                del self._cache[key]
        return None
    
    def set(self, method: str, file_path: str, line: int, char: int, result: Dict):
        key = self._make_key(method, file_path, line, char)
        self._cache[key] = (result, time.time())
        logger.info(f"[LSPCache] STORED {method} at {file_path}:{line}")
    
    def invalidate_file(self, file_path: str):
        """Invalidate all cache entries related to a file."""
        to_delete = [k for k in self._cache if file_path in k]
        for k in to_delete:
            del self._cache[k]
        if to_delete:
            logger.info(f"[LSPCache] Invalidated {len(to_delete)} entries for {file_path}")


# Global LSP Cache instance
_lsp_cache: Optional[LSPCache] = None

def get_lsp_cache() -> LSPCache:
    global _lsp_cache
    if _lsp_cache is None:
        _lsp_cache = LSPCache()
    return _lsp_cache


def _infer_language_id(path: str) -> str:
    if path.endswith(".py"):
        return "python"
    if path.endswith(".ts") or path.endswith(".tsx"):
        return "typescript"
    if path.endswith(".js") or path.endswith(".jsx"):
        return "javascript"
    if path.endswith(".cpp") or path.endswith(".hpp"):
        return "cpp"
    if path.endswith(".c"):
        return "c"
    return "plaintext"

def _default_command(language_id: str) -> str:
    if language_id == "python":
        return "pyright-langserver --stdio"
    if language_id in {"typescript", "javascript"}:
        return "typescript-language-server --stdio"
    return "clangd --stdio"

def _build_sequence(uri: str, text: str, request: Dict[str, Any], language_id: str) -> bytes:
    # Full initialization handshake required by servers
    messages = [
        {
            "jsonrpc": "2.0", 
            "id": 1, 
            "method": "initialize", 
            "params": {
                "processId": None,
                "rootUri": "file:///workspace",
                "rootPath": "/workspace",
                "capabilities": {
                    "textDocument": {
                        "rename": {"prepareSupport": True},
                        "synchronization": {"didSave": True},
                        "completion": {"completionItem": {"snippetSupport": True}},
                        "definition": {"linkSupport": True},
                        "references": {},
                        "documentSymbol": {"hierarchicalDocumentSymbolSupport": True},
                    },
                    "workspace": {
                        "applyEdit": True,
                        "workspaceEdit": {"documentChanges": True},
                        "workspaceFolders": True,
                    }
                },
                "workspaceFolders": [{"uri": "file:///workspace", "name": "workspace"}],
            }
        },
        {"jsonrpc": "2.0", "method": "initialized", "params": {}},
        {
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": language_id,
                    "version": 1,
                    "text": text,
                }
            },
        },
        request,
        {"jsonrpc": "2.0", "id": 99, "method": "shutdown", "params": {}},
        {"jsonrpc": "2.0", "method": "exit", "params": {}},
    ]
    return sequence(messages)

def _extract_response(payload: bytes, request_id: int) -> Dict[str, Any]:
    for message in decode_messages(payload):
        if message.get("id") == request_id:
            return message
    return {}

async def _ensure_scripts_async(runtime, container_id: str):
    """Ensure LSP host and connect scripts are in the container."""
    # Host script
    host_path = os.path.abspath(os.path.join(os.path.dirname(__file__), "lsp_host.py"))
    if os.path.exists(host_path):
        with open(host_path, "r") as f:
            await runtime.write_file_async(container_id, "/tmp/lsp_host.py", f.read())
    
    # Connect script
    connect_path = os.path.abspath(os.path.join(os.path.dirname(__file__), "lsp_connect.py"))
    if os.path.exists(connect_path):
        with open(connect_path, "r") as f:
            await runtime.write_file_async(container_id, "/tmp/lsp_connect.py", f.read())

def _get_socket_path(language_id: str) -> str:
    return f"/tmp/lsp_{language_id}.sock"

async def run_request_async(runtime, container_id: str, file_path: str, request: Dict[str, Any]) -> Dict[str, Any]:
    uri = f"file://{file_path}"
    text = await runtime.read_file_async(container_id, file_path)
    language_id = _infer_language_id(file_path)
    
    if LSP_MODE == "persistent":
        try:
            return await _run_persistent_request_async(runtime, container_id, file_path, uri, text, request, language_id)
        except Exception as e:
            # Automatic Fallback to Stateless
            pass
            
    # Stateless Fallback (original logic)
    payload = _build_sequence(uri, text, request, language_id)
    output = await runtime.exec_lsp_async(container_id, _default_command(language_id), payload)
    return _extract_response(output, request.get("id", 2))

async def _run_persistent_request_async(runtime, container_id, file_path, uri, text, request, language_id) -> Dict[str, Any]:
    socket_path = _get_socket_path(language_id)
    
    # Check if host is running (socket file exists)
    _, _, code = await runtime.exec_capture_async(container_id, f"ls {socket_path}")
    
    if code != 0:
        # Start Daemon
        await _ensure_scripts_async(runtime, container_id)
        cmd = _default_command(language_id)
        # Run in background via nohup to survive exec exit
        start_cmd = f"nohup python3 /tmp/lsp_host.py --socket {socket_path} --command \"{cmd}\" --ttl 300 > /tmp/lsp_{language_id}.log 2>&1 &"
        await runtime.exec_capture_async(container_id, start_cmd)
        await asyncio.sleep(1) # Give it a second to boot
    
    # Minimal sequence for persistent mode: 
    messages = [
        {
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "processId": None, "rootUri": "file:///workspace", "capabilities": {},
                "workspaceFolders": [{"uri": "file:///workspace", "name": "workspace"}]
            }
        },
        {"jsonrpc": "2.0", "method": "initialized", "params": {}},
        {
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": {"textDocument": {"uri": uri, "languageId": language_id, "version": 1, "text": text}}
        },
        request
    ]
    payload = sequence(messages)
    
    # Run via lsp_connect.py
    # We pipe payload to stdin of lsp_connect.py
    connect_cmd = f"python3 /tmp/lsp_connect.py --socket {socket_path}"
    output = await runtime.exec_lsp_async(container_id, connect_cmd, payload)
    return _extract_response(output, request.get("id", 2))


async def rename(runtime, container_id: str, file_path: str, line: int, character: int, new_name: str) -> Dict[str, Any]:
    request = {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/rename",
        "params": {
            "textDocument": {"uri": f"file://{file_path}"},
            "position": {"line": line, "character": character},
            "newName": new_name,
        },
    }
    response = await run_request_async(runtime, container_id, file_path, request)
    edit = response.get("result") or {}
    updated_contents = await _apply_edit_async(runtime, container_id, edit)
    return {"applied": True, "updated": list(updated_contents.keys())}


async def references(runtime, container_id: str, file_path: str, line: int, character: int) -> Dict[str, Any]:
    # SOTA Phase 78: Check cache first
    cache = get_lsp_cache()
    cached = cache.get("references", file_path, line, character)
    if cached:
        return cached
    
    request = {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/references",
        "params": {
            "textDocument": {"uri": f"file://{file_path}"},
            "position": {"line": line, "character": character},
            "context": {"includeDeclaration": True},
        },
    }
    response = await run_request_async(runtime, container_id, file_path, request)
    result = {"references": response.get("result") or []}
    
    # SOTA Phase 78: Store in cache
    cache.set("references", file_path, line, character, result)
    return result


async def definitions(runtime, container_id: str, file_path: str, line: int, character: int) -> Dict[str, Any]:
    # SOTA Phase 78: Check cache first
    cache = get_lsp_cache()
    cached = cache.get("definition", file_path, line, character)
    if cached:
        return cached
    
    request = {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/definition",
        "params": {
            "textDocument": {"uri": f"file://{file_path}"},
            "position": {"line": line, "character": character},
        },
    }
    response = await run_request_async(runtime, container_id, file_path, request)
    result = {"definitions": response.get("result") or []}
    
    # SOTA Phase 78: Store in cache
    cache.set("definition", file_path, line, character, result)
    return result


async def _apply_edit_async(runtime, container_id: str, edit: Dict[str, Any]) -> Dict[str, str]:
    contents: Dict[str, str] = {}
    targets: List[str] = []
    for uri in (edit.get("changes") or {}).keys():
        targets.append(uri)
    for change in edit.get("documentChanges") or []:
        if change.get("textDocument"):
            targets.append(change["textDocument"]["uri"])
    for uri in set(targets):
        path = uri.replace("file://", "")
        contents[uri] = await runtime.read_file_async(container_id, path)
    updated = apply_workspace_edit(contents, edit)
    
    # SOTA Phase 78: Invalidate cache for edited files
    cache = get_lsp_cache()
    
    for uri, text in updated.items():
        path = uri.replace("file://", "")
        await runtime.write_file_async(container_id, path, text)
        cache.invalidate_file(path)  # Invalidate on write
    return updated
