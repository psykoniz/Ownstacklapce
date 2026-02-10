import asyncio
import json
import logging
from typing import List, Dict, Any, Optional

logger = logging.getLogger(__name__)

class MCPClient:
    """
    Client for Model Context Protocol (MCP).
    Allows OwnStack to connect to external tool servers.
    """
    
    def __init__(self, server_command: str, args: List[str] = None):
        self.server_command = server_command
        self.args = args or []
        self.process: Optional[asyncio.subprocess.Process] = None
        self._id_counter = 1
        self._pending_requests: Dict[int, asyncio.Future] = {}

    async def connect(self):
        """Start the MCP server via stdio."""
        logger.info(f"Connecting to MCP server: {self.server_command} {' '.join(self.args)}")
        self.process = await asyncio.create_subprocess_exec(
            self.server_command,
            *self.args,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE
        )
        # Start reading stdout loop
        asyncio.create_task(self._read_loop())

    async def _read_loop(self):
        """Read JSON-RPC messages from the server."""
        while self.process and not self.process.stdout.at_eof():
            line = await self.process.stdout.readline()
            if not line:
                break
            try:
                message = json.loads(line.decode())
                if "id" in message and message["id"] in self._pending_requests:
                    self._pending_requests[message["id"]].set_result(message)
            except Exception as e:
                logger.error(f"MCP Read Error: {e}")

    async def _send_request(self, method: str, params: Dict[str, Any] = None) -> Dict[str, Any]:
        """Send a JSON-RPC request."""
        request_id = self._id_counter
        self._id_counter += 1
        
        request = {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params or {}
        }
        
        future = asyncio.get_event_loop().create_future()
        self._pending_requests[request_id] = future
        
        self.process.stdin.write((json.dumps(request) + "\n").encode())
        await self.process.stdin.drain()
        
        try:
            return await asyncio.wait_for(future, timeout=10)
        except asyncio.TimeoutError:
            del self._pending_requests[request_id]
            return {"error": {"message": "Request timeout"}}

    async def list_tools(self) -> List[Dict[str, Any]]:
        """List available tools on the server."""
        response = await self._send_request("tools/list")
        return response.get("result", {}).get("tools", [])

    async def call_tool(self, name: str, arguments: Dict[str, Any]) -> Dict[str, Any]:
        """Call a tool on the server."""
        response = await self._send_request("tools/call", {
            "name": name,
            "arguments": arguments
        })
        return response.get("result", {})

    async def close(self):
        """Shutdown the MCP server."""
        if self.process:
            self.process.terminate()
            await self.process.wait()
            self.process = None
