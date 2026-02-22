# Top Dettes Python Restantes

- Generated at: `2026-02-22 00:52:31Z`
- Metrics source: `.ownstack\python_bridge_metrics.jsonl`

- Total calls observed: `6`
- Total errors observed: `0`

| Rank | Endpoint | Component | Calls | Errors | Err % | Avg ms | P95 ms | Impact | Priority | Rust target |
|---:|---|---|---:|---:|---:|---:|---:|---:|---|---|
| 1 | `tools.exec` | `tools` | 2 | 0 | 0.0 | 44.0 | 88.0 | 5.5 | `P3` | `ownstack-agent/src/toolkits/` |
| 2 | `git.status` | `git` | 1 | 0 | 0.0 | 0.0 | 0.0 | 1.0 | `P3` | `ownstack-agent/src/toolkits/` |
| 3 | `lsp.hover` | `lsp` | 1 | 0 | 0.0 | 0.0 | 0.0 | 1.0 | `P3` | `ownstack-agent/src/toolkits/` |
| 4 | `agent.plan` | `agent` | 1 | 0 | 0.0 | 0.0 | 0.0 | 1.0 | `P3` | `ownstack-agent/src/orchestrator.rs` |
| 5 | `fail` | `bridge` | 1 | 0 | 0.0 | 0.0 | 0.0 | 1.0 | `P3` | `ownstack-bridge/src/lib.rs` |

Score impact utilisé: `calls + errors*6 + error_rate*100 + p95_latency_ms/25`.
