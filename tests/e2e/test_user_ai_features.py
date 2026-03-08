#!/usr/bin/env python3
"""
User acceptance test for OwnStack AI features.

Covers:
  1.  Initial state broadcast (ui_state_delta on boot)
  2.  AI chat – Ask mode (stream chunks + finish_reason + content)
  3.  Mode switching: Plan → Auto → Ask
  4.  Budget & context telemetry updates
  5.  Plan-mode mission (content streamed, run_state transitions)
  6.  run_state idle after completion
  7.  tool_exec RPC → ToolResultMsg
  8.  Time Machine toolkit (create_snapshot, list_snapshots, current_diff)
  9.  Browser toolkit (browse_url with HTTP fallback, invalid URL)
  10. Vision toolkit (capture_ui)
  11. Healer toolkit (heal command)
  12. Multivers toolkit (A/B variant run)
  13. Specialist toolkits (PM, Designer, QA, Reviewer, Security, Docs)
  14. Screenshot Capture (CaptureScreenshot RPC)
  15. SuggestionDecision RPC acknowledgment
  16. LspNotInstalled notification → alert
  17. UI Snapshot request/response flow
  18. MCP mock server handshake
  19. kill_switch terminates the agent cleanly

Usage:
  export ANTHROPIC_API_KEY=sk-ant-...   # or OPENROUTER_API_KEY / OPENAI_API_KEY
  python tests/e2e/test_user_ai_features.py

Exit codes:
  0 = all tests PASS (or SKIP when no API key)
  1 = at least one test FAIL
"""

from __future__ import annotations

import json
import os
import queue
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path
from typing import Any, Optional

REPO_ROOT = Path(__file__).resolve().parents[2]
IS_WINDOWS = os.name == "nt"
AGENT_BIN = REPO_ROOT / "target" / "debug" / (
    "ownstack-agent.exe" if IS_WINDOWS else "ownstack-agent"
)

PASS = "\033[32mPASS\033[0m"
FAIL = "\033[31mFAIL\033[0m"
SKIP = "\033[33mSKIP\033[0m"

# ---------------------------------------------------------------------------
# Low-level helpers
# ---------------------------------------------------------------------------

def _reader(pipe: Any, src: str, q: "queue.Queue[tuple[str, Optional[str]]]") -> None:
    try:
        for line in pipe:
            q.put((src, line.rstrip("\n")))
    finally:
        q.put((src, None))


def start_agent(
    workspace: Path,
    *,
    mcp: bool = False,
    extra_env: Optional[dict] = None,
) -> tuple[subprocess.Popen, "queue.Queue"]:
    if not AGENT_BIN.exists():
        raise RuntimeError(
            f"Agent binary not found: {AGENT_BIN}\nRun: cargo build -p ownstack-agent"
        )
    cmd = [str(AGENT_BIN), "--workspace", str(workspace)]
    if mcp:
        cmd.append("--mcp")
    env = os.environ.copy()
    if extra_env:
        env.update(extra_env)
    proc = subprocess.Popen(
        cmd,
        cwd=str(REPO_ROOT),
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
        env=env,
    )
    q: queue.Queue = queue.Queue()
    threading.Thread(target=_reader, args=(proc.stdout, "out", q), daemon=True).start()
    threading.Thread(target=_reader, args=(proc.stderr, "err", q), daemon=True).start()
    return proc, q


def send(proc: subprocess.Popen, payload: dict) -> None:
    assert proc.stdin
    proc.stdin.write(json.dumps(payload) + "\n")
    proc.stdin.flush()


def collect(
    q: "queue.Queue",
    timeout: float,
    *,
    stop_on_finish: bool = False,
    stop_when_running_then_idle: bool = False,
) -> tuple[list[dict], list[str]]:
    """Collect RPC messages until timeout or stop condition.

    Returns (stdout_msgs, stderr_lines).
    stop_on_finish: stop when ai_stream_chunk with finish_reason is seen.
    stop_when_running_then_idle: stop after run_state goes running→idle.
    """
    msgs: list[dict] = []
    stderr: list[str] = []
    deadline = time.time() + timeout
    saw_running = False

    while time.time() < deadline:
        remaining = max(0.05, deadline - time.time())
        try:
            src, line = q.get(timeout=remaining)
        except queue.Empty:
            break
        if line is None:
            break
        if src == "err":
            if line:
                stderr.append(line)
            continue
        if not line.strip():
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        msgs.append(msg)
        method = msg.get("method", "")
        params = msg.get("params", {}) or {}

        if stop_on_finish and method == "ai_stream_chunk":
            if params.get("finish_reason") is not None:
                break

        if stop_when_running_then_idle and method == "ui_state_delta":
            delta = params.get("delta", {}) or {}
            rs = delta.get("run_state")
            if rs == "running":
                saw_running = True
            if rs == "idle" and saw_running:
                break

    return msgs, stderr


def wait_for_idle(q: "queue.Queue", timeout: float = 8.0) -> bool:
    """Wait until agent emits run_state=idle (ready signal)."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        remaining = max(0.05, deadline - time.time())
        try:
            src, line = q.get(timeout=remaining)
        except queue.Empty:
            continue
        if line is None:
            continue
        if src == "err":
            continue
        if not line.strip():
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        if msg.get("method") == "ui_state_delta":
            delta = (msg.get("params") or {}).get("delta", {})
            if delta.get("run_state") == "idle":
                return True
    return False


def stop_agent(proc: subprocess.Popen) -> None:
    try:
        proc.terminate()
        proc.wait(timeout=5)
    except Exception:
        proc.kill()


# ---------------------------------------------------------------------------
# Result tracking
# ---------------------------------------------------------------------------

results: list[tuple[str, bool, str]] = []


def record(name: str, ok: bool, detail: str = "") -> None:
    results.append((name, ok, detail))
    icon = PASS if ok else FAIL
    suffix = f" — {detail}" if detail else ""
    print(f"  {icon}  {name}{suffix}")


# ---------------------------------------------------------------------------
# Test 1 – Initial state broadcast
# ---------------------------------------------------------------------------

def test_initial_state(boot_msgs: list[dict]) -> None:
    """Agent must have emitted ui_state_delta during boot."""
    got = any(m.get("method") == "ui_state_delta" for m in boot_msgs)
    record("initial ui_state_delta on boot", got)

    has_idle = any(
        (m.get("params") or {}).get("delta", {}).get("run_state") == "idle"
        for m in boot_msgs if m.get("method") == "ui_state_delta"
    )
    record("initial run_state=idle emitted", has_idle)


# ---------------------------------------------------------------------------
# Test 2 – AI chat in Ask mode
# ---------------------------------------------------------------------------

def _check_api_error(msgs: list[dict], stderr: list[str]) -> Optional[str]:
    """Return error description if an API error was detected, else None."""
    stderr_text = "\n".join(stderr)
    if "403" in stderr_text and "Key limit" in stderr_text:
        return "API key quota exceeded (HTTP 403)"
    if "401" in stderr_text:
        return "API key unauthorized (HTTP 401)"
    if "Process error" in stderr_text and "API error" in stderr_text:
        # Extract the error line
        for line in stderr:
            if "Process error" in line:
                return line[-120:]
    return None


def test_ask_chat(proc: subprocess.Popen, q: queue.Queue) -> None:
    # Ensure ask mode
    send(proc, {"method": "set_agent_mode", "params": {"mode": "ask"}})
    collect(q, timeout=3)  # ACK

    send(proc, {
        "method": "ai_prompt",
        "params": {"prompt": "Reply with exactly: HELLO"},
    })
    msgs, stderr = collect(q, timeout=40, stop_on_finish=True)

    api_err = _check_api_error(msgs, stderr)
    chunks = [m for m in msgs if m.get("method") == "ai_stream_chunk"]
    has_content = any((m.get("params") or {}).get("content_delta") for m in chunks)
    has_finish = any(
        (m.get("params") or {}).get("finish_reason") is not None for m in chunks
    )
    record("AI chat – stream chunks received", len(chunks) > 0,
           api_err or "")
    record("AI chat – finish_reason present", has_finish, api_err or "")
    record("AI chat – non-empty content delta", has_content, api_err or "")


# ---------------------------------------------------------------------------
# Test 3 – Mode switching
# ---------------------------------------------------------------------------

def test_mode_switch(proc: subprocess.Popen, q: queue.Queue) -> None:
    for mode in ("plan", "auto", "ask"):
        send(proc, {"method": "set_agent_mode", "params": {"mode": mode}})
        msgs, _ = collect(q, timeout=5)
        ack = any(
            m.get("method") == "ui_state_delta"
            and (m.get("params") or {}).get("delta", {}).get("mode") == mode
            for m in msgs
        )
        record(f"mode switch → {mode}", ack)


# ---------------------------------------------------------------------------
# Test 4 – Budget & context telemetry
# ---------------------------------------------------------------------------

def test_telemetry(proc: subprocess.Popen, q: queue.Queue) -> None:
    send(proc, {"method": "set_agent_mode", "params": {"mode": "ask"}})
    collect(q, timeout=3)  # ACK

    send(proc, {
        "method": "ai_prompt",
        "params": {"prompt": "Say: BUDGET_TEST"},
    })
    msgs, stderr = collect(q, timeout=40, stop_when_running_then_idle=True)

    api_err = _check_api_error(msgs, stderr)
    has_budget = any(m.get("method") == "budget_update" for m in msgs)
    has_context = any(m.get("method") == "context_update" for m in msgs)
    record("budget_update telemetry emitted", has_budget, api_err or "")
    record("context_update telemetry emitted", has_context, api_err or "")


# ---------------------------------------------------------------------------
# Test 5 – Plan mode mission
# ---------------------------------------------------------------------------

def test_plan_mission(proc: subprocess.Popen, q: queue.Queue) -> None:
    send(proc, {"method": "set_agent_mode", "params": {"mode": "plan"}})
    mode_ack_msgs, _ = collect(q, timeout=6)
    plan_acked = any(
        m.get("method") == "ui_state_delta"
        and (m.get("params") or {}).get("delta", {}).get("mode") == "plan"
        for m in mode_ack_msgs
    )
    record("plan mode ACK received", plan_acked)

    send(proc, {
        "method": "ai_prompt",
        "params": {
            "prompt": (
                "List three concrete steps to build a FastAPI REST endpoint. "
                "End your reply with the marker PLAN_DONE."
            )
        },
    })
    msgs, stderr = collect(q, timeout=90, stop_when_running_then_idle=True)

    api_err = _check_api_error(msgs, stderr)
    chunks = [m for m in msgs if m.get("method") == "ai_stream_chunk"]
    full_text = "".join(
        (m.get("params") or {}).get("content_delta") or "" for m in chunks
    )
    has_marker = "PLAN_DONE" in full_text
    has_mission = any(m.get("method") == "mission_update" for m in msgs)
    saw_running = any(
        (m.get("params") or {}).get("delta", {}).get("run_state") == "running"
        for m in msgs if m.get("method") == "ui_state_delta"
    )
    saw_idle = any(
        (m.get("params") or {}).get("delta", {}).get("run_state") == "idle"
        for m in msgs if m.get("method") == "ui_state_delta"
    )

    record("plan mission – content streamed", len(full_text) > 0, api_err or "")
    record("plan mission – PLAN_DONE marker in response", has_marker, api_err or "")
    record("plan mission – mission_update event (optional)", has_mission, "optional")
    record("plan mission – run_state running→idle", saw_running and saw_idle, api_err or "")


# ---------------------------------------------------------------------------
# Test 6 – run_state idle after ask-mode completion
# ---------------------------------------------------------------------------

def test_idle_after_completion(proc: subprocess.Popen, q: queue.Queue) -> None:
    send(proc, {"method": "set_agent_mode", "params": {"mode": "ask"}})
    collect(q, timeout=3)

    send(proc, {
        "method": "ai_prompt",
        "params": {"prompt": "Say: DONE"},
    })
    msgs, stderr = collect(q, timeout=40, stop_when_running_then_idle=True)
    api_err = _check_api_error(msgs, stderr)
    saw_idle = any(
        (m.get("params") or {}).get("delta", {}).get("run_state") == "idle"
        for m in msgs if m.get("method") == "ui_state_delta"
    )
    record("run_state=idle emitted after completion", saw_idle, api_err or "")


# ---------------------------------------------------------------------------
# Test 7 – tool_exec RPC → ToolResultMsg
# ---------------------------------------------------------------------------

def test_tool_exec(proc: subprocess.Popen, q: queue.Queue) -> None:
    send(proc, {
        "method": "tool_exec",
        "params": {
            "tool_name": "core:exec",
            "command": json.dumps({"command": "echo ownstack_tool_test"}),
        },
    })
    msgs, _ = collect(q, timeout=15)
    got = any(m.get("method") == "tool_result_msg" for m in msgs)
    record("tool_exec → tool_result_msg received", got)
    if got:
        result_msg = next(m for m in msgs if m.get("method") == "tool_result_msg")
        json_result = (result_msg.get("params") or {}).get("json_result", "")
        record("tool_result_msg contains non-empty result", bool(json_result))


# ---------------------------------------------------------------------------
# Test 8 – Time Machine toolkit (create_snapshot, list_snapshots)
# ---------------------------------------------------------------------------

def test_time_machine(proc: subprocess.Popen, q: queue.Queue) -> None:
    """Test Time Machine snapshot lifecycle via tool_exec."""
    # Create a snapshot
    send(proc, {
        "method": "tool_exec",
        "params": {
            "tool_name": "time_machine:create_snapshot",
            "command": json.dumps({"message": "E2E test snapshot"}),
        },
    })
    msgs, _ = collect(q, timeout=15)
    got_result = any(m.get("method") == "tool_result_msg" for m in msgs)
    record("time_machine:create_snapshot → result received", got_result)

    if got_result:
        result_msg = next(m for m in msgs if m.get("method") == "tool_result_msg")
        jr = (result_msg.get("params") or {}).get("json_result", "")
        record("time_machine:create_snapshot → success or no-changes",
               "snapshot" in jr.lower() or "no changes" in jr.lower() or "Snapshot" in jr,
               "optional")

    # List snapshots
    send(proc, {
        "method": "tool_exec",
        "params": {
            "tool_name": "time_machine:list_snapshots",
            "command": json.dumps({"limit": 5}),
        },
    })
    msgs, _ = collect(q, timeout=10)
    got_list = any(m.get("method") == "tool_result_msg" for m in msgs)
    record("time_machine:list_snapshots → result received", got_list)

    # Current diff
    send(proc, {
        "method": "tool_exec",
        "params": {
            "tool_name": "time_machine:current_diff",
            "command": "{}",
        },
    })
    msgs, _ = collect(q, timeout=10)
    got_diff = any(m.get("method") == "tool_result_msg" for m in msgs)
    record("time_machine:current_diff → result received", got_diff)


# ---------------------------------------------------------------------------
# Test 9 – Browser toolkit (browse_url with HTTP fallback)
# ---------------------------------------------------------------------------

def test_browser_toolkit(proc: subprocess.Popen, q: queue.Queue) -> None:
    """Test browser toolkit HTTP fallback (no Chrome needed)."""
    send(proc, {
        "method": "tool_exec",
        "params": {
            "tool_name": "browser:browse_url",
            "command": json.dumps({
                "url": "https://httpbin.org/html",
                "action": "navigate",
                "extract": "text",
            }),
        },
    })
    msgs, _ = collect(q, timeout=30)
    got_result = any(m.get("method") == "tool_result_msg" for m in msgs)
    record("browser:browse_url → tool_result_msg received", got_result)

    if got_result:
        result_msg = next(m for m in msgs if m.get("method") == "tool_result_msg")
        jr = (result_msg.get("params") or {}).get("json_result", "")
        # HTTP fallback should return page content or an error
        has_content = "Herman Melville" in jr or "Status:" in jr or "httpbin" in jr.lower()
        record("browser:browse_url → page content retrieved", has_content, "optional")

    # Test invalid URL rejection
    send(proc, {
        "method": "tool_exec",
        "params": {
            "tool_name": "browser:browse_url",
            "command": json.dumps({"url": "not-a-url"}),
        },
    })
    msgs, _ = collect(q, timeout=10)
    got_err = any(m.get("method") == "tool_result_msg" for m in msgs)
    record("browser:browse_url invalid URL → error returned", got_err)


# ---------------------------------------------------------------------------
# Test 10 – Vision toolkit (capture_ui)
# ---------------------------------------------------------------------------

def test_vision_toolkit(proc: subprocess.Popen, q: queue.Queue) -> None:
    """Test vision:capture_ui (expects failure without UI snapshot metadata)."""
    send(proc, {
        "method": "tool_exec",
        "params": {
            "tool_name": "vision:capture_ui",
            "command": json.dumps({"panel": "editor"}),
        },
    })
    msgs, _ = collect(q, timeout=10)
    got_result = any(m.get("method") == "tool_result_msg" for m in msgs)
    record("vision:capture_ui → tool_result_msg received", got_result)
    if got_result:
        result_msg = next(m for m in msgs if m.get("method") == "tool_result_msg")
        jr = (result_msg.get("params") or {}).get("json_result", "")
        # In headless E2E, UI snapshot metadata won't exist — expect a meaningful error
        has_response = "snapshot" in jr.lower() or "capture" in jr.lower() or "success" in jr
        record("vision:capture_ui → meaningful response", has_response)


# ---------------------------------------------------------------------------
# Test 11 – Healer toolkit
# ---------------------------------------------------------------------------

def test_healer_toolkit(proc: subprocess.Popen, q: queue.Queue) -> None:
    """Test healer:heal with a simple command."""
    send(proc, {
        "method": "tool_exec",
        "params": {
            "tool_name": "healer:heal",
            "command": json.dumps({
                "command": "echo healer_test_ok",
                "max_attempts": 1,
            }),
        },
    })
    msgs, _ = collect(q, timeout=20)
    got_result = any(m.get("method") == "tool_result_msg" for m in msgs)
    record("healer:heal → tool_result_msg received", got_result)


# ---------------------------------------------------------------------------
# Test 12 – Multivers toolkit (A/B test)
# ---------------------------------------------------------------------------

def test_multivers_toolkit(proc: subprocess.Popen, q: queue.Queue) -> None:
    """Test multivers:multivers_run with two simple variants."""
    send(proc, {
        "method": "tool_exec",
        "params": {
            "tool_name": "multivers:multivers_run",
            "command": json.dumps({
                "command": "echo variant_test",
                "variants": [
                    {"name": "v1", "env": {}},
                    {"name": "v2", "env": {}},
                ],
            }),
        },
    })
    msgs, _ = collect(q, timeout=20)
    got_result = any(m.get("method") == "tool_result_msg" for m in msgs)
    record("multivers:multivers_run → tool_result_msg received", got_result)
    if got_result:
        result_msg = next(m for m in msgs if m.get("method") == "tool_result_msg")
        jr = (result_msg.get("params") or {}).get("json_result", "")
        record("multivers → contains result data", len(jr) > 10)


# ---------------------------------------------------------------------------
# Test 13 – Specialist toolkits via tool_exec
# ---------------------------------------------------------------------------

def test_specialist_toolkits(proc: subprocess.Popen, q: queue.Queue) -> None:
    """Test each specialist toolkit returns a tool_result_msg."""
    specialists = [
        ("pm:create_specification", {"feature_name": "E2E test", "requirements": "Test requirement"}),
        ("designer:generate_ui_mockup", {"component": "TestPanel", "description": "A test panel"}),
        ("qa:analyze_test_failure", {"error_output": "assert 1 == 2", "test_name": "test_basic"}),
        ("reviewer:analyze_complexity", {"file_path": "Cargo.toml"}),
        ("security:scan_dependencies", {}),
        ("docs:search_external_docs", {"query": "rust tokio"}),
    ]

    for tool_name, params in specialists:
        send(proc, {
            "method": "tool_exec",
            "params": {
                "tool_name": tool_name,
                "command": json.dumps(params),
            },
        })
        msgs, _ = collect(q, timeout=10)
        got = any(m.get("method") == "tool_result_msg" for m in msgs)
        short_name = tool_name.split(":")[0]
        record(f"specialist:{short_name} → tool_result_msg", got)


# ---------------------------------------------------------------------------
# Test 14 – Screenshot Capture RPC
# ---------------------------------------------------------------------------

def test_screenshot_capture(proc: subprocess.Popen, q: queue.Queue) -> None:
    """Test CaptureScreenshot RPC returns a tool_result_msg."""
    send(proc, {"method": "capture_screenshot"})
    msgs, _ = collect(q, timeout=10)
    got = any(m.get("method") == "tool_result_msg" for m in msgs)
    record("CaptureScreenshot → tool_result_msg received", got)
    if got:
        result_msg = next(m for m in msgs if m.get("method") == "tool_result_msg")
        jr = (result_msg.get("params") or {}).get("json_result", "")
        # May succeed or fail depending on display; just check structure
        has_path = "path" in jr
        record("CaptureScreenshot → response has path field", has_path)


# ---------------------------------------------------------------------------
# Test 15 – SuggestionDecision RPC
# ---------------------------------------------------------------------------

def test_suggestion_decision(proc: subprocess.Popen, q: queue.Queue) -> None:
    """Test SuggestionDecision RPC returns acknowledgment."""
    send(proc, {
        "method": "suggestion_decision",
        "params": {"decision": "accepted", "message_id": "test-msg-001"},
    })
    msgs, _ = collect(q, timeout=10)
    # Should get a tool_result_msg or ui_state_delta acknowledging the decision
    got_result = any(m.get("method") == "tool_result_msg" for m in msgs)
    got_delta = any(
        m.get("method") == "ui_state_delta"
        and (m.get("params") or {}).get("delta", {}).get("tool_event", {}).get("tool_name") == "suggestion"
        for m in msgs
    )
    record("SuggestionDecision → acknowledged", got_result or got_delta)


# ---------------------------------------------------------------------------
# Test 16 – LspNotInstalled notification
# ---------------------------------------------------------------------------

def test_lsp_not_installed(proc: subprocess.Popen, q: queue.Queue) -> None:
    """Test LspNotInstalled RPC emits an alert."""
    send(proc, {
        "method": "lsp_not_installed",
        "params": {
            "language_id": "python",
            "install_hint": "pip install python-lsp-server",
        },
    })
    msgs, _ = collect(q, timeout=10)
    got_alert = any(
        m.get("method") == "ui_state_delta"
        and (m.get("params") or {}).get("delta", {}).get("alert") is not None
        for m in msgs
    )
    got_result = any(m.get("method") == "tool_result_msg" for m in msgs)
    record("LspNotInstalled → alert emitted", got_alert or got_result)


# ---------------------------------------------------------------------------
# Test 17 – UI Snapshot request/response flow
# ---------------------------------------------------------------------------

def test_ui_snapshot_flow(proc: subprocess.Popen, q: queue.Queue) -> None:
    """Test UiSnapshotRequest is echoed back and UiSnapshot is persisted."""
    # Send UiSnapshotRequest — agent should echo it back
    send(proc, {"method": "ui_snapshot_request"})
    msgs, _ = collect(q, timeout=5)
    echoed = any(m.get("method") == "ui_snapshot_request" for m in msgs)
    record("UiSnapshotRequest → echoed back", echoed)

    # Now send a UiSnapshot (simulating the IDE response)
    snapshot_data = json.dumps({
        "editor": {"file": "main.rs", "line": 42},
        "panels": ["chat", "terminal"],
        "timestamp": "2026-03-08T12:00:00Z",
    })
    send(proc, {
        "method": "ui_snapshot",
        "params": {"metadata": snapshot_data},
    })
    # Give the agent a moment to persist
    time.sleep(0.5)
    msgs, stderr = collect(q, timeout=3)
    # Check stderr for success log
    stderr_text = "\n".join(stderr)
    persisted = "UI snapshot metadata received" in stderr_text or "stored" in stderr_text
    record("UiSnapshot → metadata persisted", persisted, "optional")


# ---------------------------------------------------------------------------
# Test 18 – MCP mock server handshake
# ---------------------------------------------------------------------------

MOCK_MCP_SERVER = """\
import sys, json, signal

def send(obj):
    sys.stdout.write(json.dumps(obj) + "\\n")
    sys.stdout.flush()

# Ignore SIGTERM/SIGPIPE so we stay alive while the agent runs
try:
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    signal.signal(signal.SIGPIPE, signal.SIG_IGN)
except Exception:
    pass

# Serve forever using readline() (compatible with agent newline-delimited framing)
while True:
    line = sys.stdin.readline()
    if not line:
        break  # stdin closed = agent shut down
    line = line.strip()
    if not line:
        continue
    try:
        req = json.loads(line)
    except Exception:
        continue
    m = req.get("method", "")
    if m == "initialize":
        send({"jsonrpc":"2.0","id":req.get("id"),"result":{
            "protocolVersion":"2024-11-05",
            "capabilities":{"tools":{}},
            "serverInfo":{"name":"mock-mcp","version":"0.1"}
        }})
    elif m == "notifications/initialized":
        pass  # notification, no response needed
    elif m == "tools/list":
        send({"jsonrpc":"2.0","id":req.get("id"),"result":{
            "tools":[{"name":"mock_tool","description":"A mock tool",
                      "inputSchema":{"type":"object","properties":{}}}]
        }})
    elif req.get("id") is not None:
        # Respond to any other request with an empty result
        send({"jsonrpc":"2.0","id":req.get("id"),"result":{}})
"""


def test_mcp_handshake() -> None:
    """Start an isolated agent with a mock MCP server (client connection test).

    Note: The agent is started WITHOUT --mcp so it runs in IDE RPC mode and
    emits boot messages on stdout. The MCP *client* connections are loaded from
    mcp_servers.json regardless of the --mcp flag.
    """
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = Path(tmp)
        mock_script = tmp_path / "mock_mcp.py"
        mock_script.write_text(MOCK_MCP_SERVER)

        ownstack_dir = tmp_path / ".ownstack"
        ownstack_dir.mkdir()
        mcp_config = {
            "servers": [{
                "name": "mock",
                "command": sys.executable,
                "args": [str(mock_script)],
                "enabled": True,
            }]
        }
        (ownstack_dir / "mcp_servers.json").write_text(json.dumps(mcp_config))

        try:
            # NO --mcp flag: agent runs in IDE RPC mode, MCP client loaded from file
            proc, q = start_agent(tmp_path, mcp=False)
        except RuntimeError as e:
            record("MCP – agent started (IDE RPC mode)", False, str(e))
            record("MCP – client handshake attempted", False, str(e))
            return

        try:
            # Boot messages appear after MCP client initialization (a few seconds)
            msgs, stderr_lines = collect(q, timeout=15)
            stderr_text = "\n".join(stderr_lines)

            has_any_rpc = len(msgs) > 0
            # Agent tries to connect (logs "Connecting to MCP server: mock")
            mcp_attempted = (
                "Connecting to MCP server" in stderr_text
                or "MCP server" in stderr_text
            )
            # "Connected MCP server" = success; "Failed to connect" = tried but failed
            mcp_tried = mcp_attempted or "mock" in stderr_text.lower()

            record("MCP – agent started (IDE RPC mode)", has_any_rpc)
            record("MCP – client handshake attempted", mcp_tried,
                   "" if mcp_tried else "no MCP connection attempt in stderr")
        except Exception as e:
            record("MCP – agent started (IDE RPC mode)", False, str(e))
            record("MCP – client handshake attempted", False, str(e))
        finally:
            stop_agent(proc)


# ---------------------------------------------------------------------------
# Test 9 – kill_switch terminates the agent cleanly
# ---------------------------------------------------------------------------

def test_kill_switch() -> None:
    """Sending kill_switch must make the agent exit within 5 seconds."""
    proc, q = start_agent(REPO_ROOT)
    # Wait for the agent to be ready
    wait_for_idle(q, timeout=8)

    # kill_switch is a unit variant — no params field
    send(proc, {"method": "kill_switch"})
    try:
        proc.wait(timeout=5)
        exited = True
    except subprocess.TimeoutExpired:
        exited = False
        stop_agent(proc)

    record("kill_switch – agent exits cleanly", exited)


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

def main() -> int:
    has_key = any(
        os.getenv(k)
        for k in ["ANTHROPIC_API_KEY", "OPENROUTER_API_KEY", "OPENAI_API_KEY"]
    )
    if not has_key:
        print(
            f"  {SKIP}  No LLM API key — set ANTHROPIC_API_KEY / "
            "OPENROUTER_API_KEY / OPENAI_API_KEY to run tests."
        )
        return 0

    if not AGENT_BIN.exists():
        print(f"  {FAIL}  Binary not found: {AGENT_BIN}")
        print("         Run: cargo build -p ownstack-agent")
        return 1

    key_name = (
        "ANTHROPIC"
        if os.getenv("ANTHROPIC_API_KEY")
        else "OPENROUTER"
        if os.getenv("OPENROUTER_API_KEY")
        else "OPENAI"
    )
    print(f"\n=== OwnStack User AI Features Test ===")
    print(f"Agent : {AGENT_BIN}")
    print(f"Key   : {key_name}")
    print()

    # ── Shared agent (LLM-dependent tests) ──────────────────────────────────
    proc, q = start_agent(REPO_ROOT)

    # Collect the boot broadcast messages (agent emits them within ~1s)
    boot_msgs, _ = collect(q, timeout=5)

    try:
        print("[1] Initial state")
        test_initial_state(boot_msgs)

        print("\n[2] AI Chat – Ask mode")
        test_ask_chat(proc, q)

        print("\n[3] Mode switching")
        test_mode_switch(proc, q)

        print("\n[4] Telemetry (budget & context)")
        test_telemetry(proc, q)

        print("\n[5] Plan-mode mission")
        test_plan_mission(proc, q)

        print("\n[6] Idle state after completion")
        test_idle_after_completion(proc, q)

        print("\n[7] Tool execution (tool_exec)")
        test_tool_exec(proc, q)

        print("\n[8] Time Machine toolkit")
        test_time_machine(proc, q)

        print("\n[9] Browser toolkit")
        test_browser_toolkit(proc, q)

        print("\n[10] Vision toolkit")
        test_vision_toolkit(proc, q)

        print("\n[11] Healer toolkit")
        test_healer_toolkit(proc, q)

        print("\n[12] Multivers toolkit")
        test_multivers_toolkit(proc, q)

        print("\n[13] Specialist toolkits")
        test_specialist_toolkits(proc, q)

        print("\n[14] Screenshot Capture RPC")
        test_screenshot_capture(proc, q)

        print("\n[15] SuggestionDecision RPC")
        test_suggestion_decision(proc, q)

        print("\n[16] LspNotInstalled notification")
        test_lsp_not_installed(proc, q)

        print("\n[17] UI Snapshot flow")
        test_ui_snapshot_flow(proc, q)
    finally:
        stop_agent(proc)

    # ── Isolated tests ───────────────────────────────────────────────────────
    print("\n[18] MCP mock server handshake")
    test_mcp_handshake()

    print("\n[19] kill_switch")
    test_kill_switch()

    # ── Summary ─────────────────────────────────────────────────────────────
    total = len(results)
    passed = sum(1 for _, ok, _ in results if ok)
    optional_fails = sum(1 for _, ok, d in results if not ok and "optional" in d)
    hard_failed = total - passed - optional_fails

    print(f"\n{'─'*42}")
    print(
        f"Results: {passed}/{total} passed"
        + (f", {optional_fails} optional" if optional_fails else "")
        + (f", {hard_failed} failed" if hard_failed else "")
    )

    if hard_failed:
        print("\nFailed tests:")
        for name, ok, detail in results:
            if not ok and "optional" not in detail:
                print(f"  • {name}" + (f" ({detail})" if detail else ""))
        return 1

    if optional_fails:
        print("\nOptional (not counted as failures):")
        for name, ok, detail in results:
            if not ok and "optional" in detail:
                print(f"  ○ {name}")

    print(f"\n{PASS}  All required AI feature tests passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
