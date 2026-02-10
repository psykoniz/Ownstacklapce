"""JSON-RPC framing utilities."""
from __future__ import annotations

import json
from typing import Any, Dict, Iterable, List, Tuple


def encode_message(message: Dict[str, Any]) -> bytes:
    body = json.dumps(message, separators=(",", ":")).encode("utf-8")
    header = f"Content-Length: {len(body)}\r\n\r\n".encode("utf-8")
    return header + body


def decode_messages(payload: bytes) -> Iterable[Dict[str, Any]]:
    cursor = 0
    while cursor < len(payload):
        header_end = payload.find(b"\r\n\r\n", cursor)
        if header_end == -1:
            break
        header = payload[cursor:header_end].decode("utf-8", errors="replace")
        length = 0
        for line in header.split("\r\n"):
            if line.lower().startswith("content-length"):
                length = int(line.split(":", 1)[1].strip())
                break
        body_start = header_end + 4
        body_end = body_start + length
        body = payload[body_start:body_end]
        cursor = body_end
        if body:
            yield json.loads(body)


def sequence(messages: List[Dict[str, Any]]) -> bytes:
    return b"".join(encode_message(message) for message in messages)
