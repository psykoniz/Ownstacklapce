#!/usr/bin/env python3
"""Minimal MCP server fixture for CI integration tests.

Implements JSON-RPC 2.0 over newline-delimited stdio (MCP protocol-2024-11-05).
Exposes two fake tools so the integration test can exercise the full
connect → tools/list → tools/call round-trip without any network access.
"""
import sys
import json
import logging

# Logging to stderr so it never contaminates the JSON-RPC stdout stream.
logging.basicConfig(stream=sys.stderr, level=logging.INFO, format="[mock-mcp] %(message)s")

FAKE_TOOLS = [
    {
        "name": "echo",
        "description": "Returns the input text unchanged.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "text": {"type": "string", "description": "Text to echo"}
            },
            "required": ["text"],
        },
    },
    {
        "name": "add",
        "description": "Returns the sum of two integers.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "a": {"type": "integer"},
                "b": {"type": "integer"},
            },
            "required": ["a", "b"],
        },
    },
]


def send(obj):
    line = json.dumps(obj)
    sys.stdout.write(line + "\n")
    sys.stdout.flush()
    logging.info("sent: %s", line)


def handle(msg):
    method = msg.get("method", "")
    req_id = msg.get("id")
    params = msg.get("params") or {}
    logging.info("recv method=%s id=%s", method, req_id)

    if method == "initialize":
        send({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {"listChanged": False}},
                "serverInfo": {"name": "mock-mcp-fixture", "version": "0.1.0"},
            },
        })

    elif method == "notifications/initialized":
        # Notification — no response
        pass

    elif method == "tools/list":
        send({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {"tools": FAKE_TOOLS},
        })

    elif method == "tools/call":
        tool_name = params.get("name", "")
        args = params.get("arguments") or {}

        if tool_name == "echo":
            text = args.get("text", "")
            send({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "content": [{"type": "text", "text": text}],
                    "isError": False,
                },
            })

        elif tool_name == "add":
            result = int(args.get("a", 0)) + int(args.get("b", 0))
            send({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "content": [{"type": "text", "text": str(result)}],
                    "isError": False,
                },
            })

        else:
            send({
                "jsonrpc": "2.0",
                "id": req_id,
                "error": {"code": -32601, "message": f"Tool not found: {tool_name}"},
            })

    else:
        if req_id is not None:
            send({
                "jsonrpc": "2.0",
                "id": req_id,
                "error": {"code": -32601, "message": f"Method not found: {method}"},
            })


def main():
    logging.info("mock MCP server starting on stdio")
    for raw_line in sys.stdin:
        raw_line = raw_line.strip()
        if not raw_line:
            continue
        try:
            msg = json.loads(raw_line)
            handle(msg)
        except json.JSONDecodeError as exc:
            logging.error("JSON parse error: %s", exc)
            send({
                "jsonrpc": "2.0",
                "id": None,
                "error": {"code": -32700, "message": f"Parse error: {exc}"},
            })
    logging.info("stdin closed, exiting")


if __name__ == "__main__":
    main()
