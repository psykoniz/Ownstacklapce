"""Pydantic tool definitions for agent tool calling."""
from __future__ import annotations

from typing import Any, Dict, List, Optional, Type

from pydantic import BaseModel, Field


class ToolDefinition(BaseModel):
    name: str = Field(..., min_length=1)
    description: str = Field(default="")
    parameters: Dict[str, Any] = Field(default_factory=dict)


class ReadFileArgs(BaseModel):
    path: str = Field(..., min_length=1)
    start_line: Optional[int] = Field(default=None, ge=1)
    end_line: Optional[int] = Field(default=None, ge=1)


class WriteFileArgs(BaseModel):
    path: str = Field(..., min_length=1)
    content: str


class ApplyPatchArgs(BaseModel):
    path: str = Field(..., min_length=1)
    unified_diff: str


class ExecuteCommandArgs(BaseModel):
    cmd: str = Field(..., min_length=1)


class LspRenameArgs(BaseModel):
    path: str = Field(..., min_length=1)
    line: int = Field(..., ge=0)
    col: int = Field(..., ge=0)
    new_name: str = Field(..., min_length=1)


class BrowseUrlArgs(BaseModel):
    url: str = Field(..., min_length=1)
    action: Optional[str] = Field(default="navigate", description="click, type, or navigate")
    selector: Optional[str] = Field(default=None)
    text: Optional[str] = Field(default=None)


class DelegateTaskArgs(BaseModel):
    role: str = Field(..., description="The role of the sub-agent (e.g., 'Security Auditor', 'Performance Expert')")
    instructions: str = Field(..., description="The task to perform")


class SearchDirArgs(BaseModel):
    query: str = Field(..., min_length=1)
    path: Optional[str] = Field(default=".", description="Path to search in")


def tool_from_model(name: str, description: str, model: Type[BaseModel]) -> ToolDefinition:
    return ToolDefinition(
        name=name,
        description=description,
        parameters=model.model_json_schema(),
    )


DEFAULT_TOOLS: List[ToolDefinition] = [
    tool_from_model(
        name="read_file",
        description="Read a file from the workspace.",
        model=ReadFileArgs,
    ),
    tool_from_model(
        name="write_file",
        description="Write content to a file in the workspace.",
        model=WriteFileArgs,
    ),
    tool_from_model(
        name="apply_patch",
        description="Apply a unified diff patch to a file in the workspace.",
        model=ApplyPatchArgs,
    ),
    tool_from_model(
        name="execute_command",
        description="Execute a shell command inside the sandbox.",
        model=ExecuteCommandArgs,
    ),
    tool_from_model(
        name="lsp_rename",
        description="Run an LSP rename operation in the workspace.",
        model=LspRenameArgs,
    ),
    tool_from_model(
        name="browse_url",
        description="Safely browse a URL and perform an action (navigate, click, type).",
        model=BrowseUrlArgs,
    ),
    tool_from_model(
        name="delegate_task",
        description="Delegate a specialized task to a sub-agent.",
        model=DelegateTaskArgs,
    ),
    tool_from_model(
        name="search_dir",
        description="Search for a string in a directory recursively. Returns a succinct list of files.",
        model=SearchDirArgs,
    ),
]
