"""LSP Hub - Persistent LSP server pool for improved performance."""
from __future__ import annotations

import asyncio
import json
import time
import uuid
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Tuple
from collections import defaultdict

from app.tools.lsp.framing import decode_messages, encode_message


@dataclass
class DocumentState:
    """Tracks state of an open document in an LSP server."""
    uri: str
    version: int
    content_hash: str
    last_access: float


@dataclass
class LspServerState:
    """Tracks state of a single LSP server process."""
    server_id: str
    language_id: str
    container_id: str
    command: str
    initialized: bool = False
    documents: Dict[str, DocumentState] = field(default_factory=dict)  # uri -> state
    pending_requests: Dict[int, asyncio.Future] = field(default_factory=dict)
    next_request_id: int = 1
    created_at: float = field(default_factory=time.time)
    last_request: float = field(default_factory=time.time)
    request_count: int = 0
    error_count: int = 0
    
    @property
    def age_seconds(self) -> float:
        return time.time() - self.created_at
    
    @property
    def idle_seconds(self) -> float:
        return time.time() - self.last_request


class LspHub:
    """
    Manages a pool of persistent LSP servers per language/container.
    
    Benefits over stateless approach:
    - No initialize/shutdown overhead per request
    - Server can build and cache project index
    - Faster rename/references operations
    - Document state reuse across requests
    """
    
    # Server TTL: recycle after 30 minutes of inactivity
    SERVER_TTL_SECONDS = 30 * 60
    # Max servers per container
    MAX_SERVERS_PER_CONTAINER = 5
    
    def __init__(self, runtime):
        self.runtime = runtime
        self._servers: Dict[str, LspServerState] = {}  # key: f"{container_id}:{language_id}"
        self._lock = asyncio.Lock()
    
    def _server_key(self, container_id: str, language_id: str) -> str:
        return f"{container_id}:{language_id}"
    
    def _default_command(self, language_id: str) -> str:
        commands = {
            "python": "pyright-langserver --stdio",
            "typescript": "typescript-language-server --stdio",
            "javascript": "typescript-language-server --stdio",
            "cpp": "clangd --stdio",
            "c": "clangd --stdio",
        }
        return commands.get(language_id, "typescript-language-server --stdio")
    
    async def get_or_create_server(
        self, 
        container_id: str, 
        language_id: str
    ) -> LspServerState:
        """Get existing server or create a new one."""
        key = self._server_key(container_id, language_id)
        
        async with self._lock:
            # Check for existing server
            if key in self._servers:
                server = self._servers[key]
                # Check if expired
                if server.idle_seconds > self.SERVER_TTL_SECONDS:
                    del self._servers[key]
                else:
                    server.last_request = time.time()
                    return server
            
            # Cleanup expired servers for this container
            await self._cleanup_expired(container_id)
            
            # Create new server state
            server = LspServerState(
                server_id=uuid.uuid4().hex,
                language_id=language_id,
                container_id=container_id,
                command=self._default_command(language_id),
            )
            self._servers[key] = server
            return server
    
    async def _cleanup_expired(self, container_id: str) -> None:
        """Remove expired servers for a container."""
        keys_to_remove = []
        for key, server in self._servers.items():
            if server.container_id == container_id:
                if server.idle_seconds > self.SERVER_TTL_SECONDS:
                    keys_to_remove.append(key)
        for key in keys_to_remove:
            del self._servers[key]
    
    def mark_document_open(
        self, 
        container_id: str, 
        language_id: str, 
        uri: str, 
        version: int,
        content_hash: str
    ) -> None:
        """Track an open document for cache hit detection."""
        key = self._server_key(container_id, language_id)
        if key in self._servers:
            self._servers[key].documents[uri] = DocumentState(
                uri=uri,
                version=version,
                content_hash=content_hash,
                last_access=time.time(),
            )
    
    def is_document_cached(
        self, 
        container_id: str, 
        language_id: str, 
        uri: str,
        content_hash: str
    ) -> bool:
        """Check if a document is already open and up-to-date."""
        key = self._server_key(container_id, language_id)
        if key not in self._servers:
            return False
        server = self._servers[key]
        if uri not in server.documents:
            return False
        return server.documents[uri].content_hash == content_hash
    
    def record_request(self, container_id: str, language_id: str, success: bool = True) -> None:
        """Record a request for statistics."""
        key = self._server_key(container_id, language_id)
        if key in self._servers:
            self._servers[key].request_count += 1
            self._servers[key].last_request = time.time()
            if not success:
                self._servers[key].error_count += 1
    
    def shutdown_server(self, container_id: str, language_id: str) -> None:
        """Remove server from pool (container will handle process cleanup)."""
        key = self._server_key(container_id, language_id)
        self._servers.pop(key, None)
    
    def shutdown_all_for_container(self, container_id: str) -> None:
        """Remove all servers for a container."""
        keys_to_remove = [
            k for k in self._servers 
            if k.startswith(f"{container_id}:")
        ]
        for key in keys_to_remove:
            del self._servers[key]
    
    def get_stats(self) -> Dict[str, Any]:
        """Get hub statistics."""
        return {
            "active_servers": len(self._servers),
            "total_requests": sum(s.request_count for s in self._servers.values()),
            "total_errors": sum(s.error_count for s in self._servers.values()),
            "servers": [
                {
                    "key": key,
                    "language": s.language_id,
                    "initialized": s.initialized,
                    "open_documents": len(s.documents),
                    "age_seconds": int(s.age_seconds),
                    "idle_seconds": int(s.idle_seconds),
                    "request_count": s.request_count,
                }
                for key, s in self._servers.items()
            ]
        }
    
    async def warm_up(self, container_id: str, languages: List[str]) -> None:
        """Pre-initialize LSP servers for common languages."""
        for lang in languages:
            await self.get_or_create_server(container_id, lang)


# Global hub instance
_hub: Optional[LspHub] = None


def get_hub(runtime) -> LspHub:
    """Get or create the global LSP hub."""
    global _hub
    if _hub is None:
        _hub = LspHub(runtime)
    return _hub


def reset_hub() -> None:
    """Reset the global hub (for testing)."""
    global _hub
    _hub = None

