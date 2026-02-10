"""WorkspaceEdit application with UTF-16 safe offsets."""
from __future__ import annotations

from typing import Dict, List


def _codepoint_index_from_utf16(line_text: str, character: int) -> int:
    if character <= 0:
        return 0
    count = 0
    for idx, char in enumerate(line_text):
        count += len(char.encode("utf-16-le")) // 2
        if count >= character:
            return idx + 1
    return len(line_text)


def _offset_from_position(text: str, line: int, character: int) -> int:
    lines = text.splitlines(keepends=True)
    if line >= len(lines):
        return len(text)
    prefix = "".join(lines[:line])
    line_text = lines[line]
    codepoint_index = _codepoint_index_from_utf16(line_text, character)
    return len(prefix) + codepoint_index


def apply_text_edits(text: str, edits: List[Dict]) -> str:
    sorted_edits = sorted(
        edits,
        key=lambda e: (e["range"]["start"]["line"], e["range"]["start"]["character"]),
        reverse=True,
    )
    for edit in sorted_edits:
        start = edit["range"]["start"]
        end = edit["range"]["end"]
        start_offset = _offset_from_position(text, start["line"], start["character"])
        end_offset = _offset_from_position(text, end["line"], end["character"])
        text = text[:start_offset] + edit.get("newText", "") + text[end_offset:]
    return text


def apply_workspace_edit(contents: Dict[str, str], edit: Dict) -> Dict[str, str]:
    updated = dict(contents)
    changes = edit.get("changes") or {}
    for uri, edits in changes.items():
        if uri not in updated:
            continue
        updated[uri] = apply_text_edits(updated[uri], edits)
    for change in edit.get("documentChanges") or []:
        if change.get("textDocument"):
            uri = change["textDocument"]["uri"]
            edits = change.get("edits", [])
            if uri in updated:
                updated[uri] = apply_text_edits(updated[uri], edits)
    return updated
