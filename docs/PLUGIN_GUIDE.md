# OwnStack Toolkit Developer Guide

## Overview

This guide explains how to create custom toolkits for OwnStack IDE's AI agent system.

## Architecture

```
Agent → Toolkit → Tool → Security Pipeline → Execution
```

Every tool execution passes through:
1. **PolicyEngine** — Auto / Ask / Blocked decision
2. **PathValidator** — Ensures paths stay within workspace
3. **ProcessSandbox** — Isolated execution environment
4. **AuditLogger** — Full action traceability

## Creating a Toolkit

### 1. Implement the `Toolkit` trait

```rust
use async_trait::async_trait;
use ownstack_agent::toolkits::{ToolDef, ToolResult, Toolkit, ToolkitError};

pub struct MyToolkit {
    workspace: PathBuf,
}

#[async_trait]
impl Toolkit for MyToolkit {
    fn name(&self) -> &str {
        "my_toolkit"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef {
                name: "my_tool".to_string(),
                description: "Description for the LLM".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "input": {
                            "type": "string",
                            "description": "What this parameter does"
                        }
                    },
                    "required": ["input"]
                }),
            },
        ]
    }

    async fn execute(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<ToolResult, ToolkitError> {
        match tool_name {
            "my_tool" => {
                let input = args.get("input")
                    .and_then(|v| v.as_str())
                    .ok_or(ToolkitError::InvalidArguments("missing input".into()))?;
                
                // Your implementation here
                Ok(ToolResult::success(format!("Result: {}", input)))
            }
            _ => Err(ToolkitError::ToolNotFound(tool_name.to_string())),
        }
    }
}
```

### 2. Register with the Agent

```rust
use std::sync::Arc;

let toolkit = Arc::new(MyToolkit::new(workspace));
orchestrator.register_toolkit(toolkit);
```

## Security Requirements

All toolkits MUST follow these rules:

| Rule | Description |
|------|-------------|
| **PolicyEngine** | All shell commands go through `PolicyEngine::evaluate()` |
| **PathValidator** | All file paths validated via `PathValidator::validate()` |
| **Sandbox** | Shell execution via `ProcessSandbox` only |
| **No secrets** | Never hardcode API keys or credentials |
| **No unsafe** | No `unsafe {}` without documented `// SAFETY:` |

## Available Toolkits

| Toolkit | Module | Tools |
|---------|--------|-------|
| **Core** | `toolkits::core` | exec, read, write, search |
| **LSP** | `toolkits::lsp` | diagnostics, goto_definition, find_references, symbols |
| **Git** | `toolkits::git` | status, diff, log, stage, commit, branches |
| **MCP** | `toolkits::mcp` | Dynamically discovered from MCP servers |

## MCP Integration

### As Client (consuming external tools)

```rust
use ownstack_agent::toolkits::mcp::{McpToolkit, McpServerConfig};

let mut mcp = McpToolkit::new();
mcp.add_server(McpServerConfig {
    name: "my-server".to_string(),
    command: "npx".to_string(),
    args: vec!["-y".to_string(), "my-mcp-server".to_string()],
    env: HashMap::new(),
}).await?;
```

### As Server (exposing OwnStack tools)

```rust
use ownstack_agent::McpServer;

let mut server = McpServer::new("ownstack", "0.1.0");
server.register_toolkit(core_toolkit);
server.run_stdio().await?;
```

## Multi-Agent System

The orchestrator supports three agent roles:

- **Planner** — Decomposes tasks into atomic steps
- **Worker** — Executes steps using tools
- **Critic** — Reviews output for correctness

```rust
// Full mission execution
let result = orchestrator.execute_mission("Fix the login bug").await?;
```
