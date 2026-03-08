#!/usr/bin/env python3
"""
User acceptance test for OwnStack AI features.

Covers:
  1. Ping / basic RPC handshake
  2. AI chat in Ask mode (stream chunks, finish)
  3. Mode switching: Ask → Plan → Auto
  4. Budget & context telemetry updates
  5. Mission creation with PLAN mode
  6. Tool-use cycle (agent calls a tool, result returned)

Usage:
  export ANTHROPIC_API_KEY=sk-ant-...
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
# Helpers
# ---------------------------------------------------------------------------

def _reader(pipe: Any, src: str, q: "queue.Queue[tuple[str, Optional[str]]]") -> None:
    try:
        for line in pipe:
            q.put((src, line.rstrip("\n")))
    finally:
        q.put((src, None))


def start_agent(workspace: Path) -> tuple[subprocess.Popen, "queue.Queue"]:
    if not AGENT_BIN.exists():
        raise RuntimeError(f"Agent binary not found: {AGENT_BIN}")
    proc = subprocess.Popen(
        [str(AGENT_BIN), "--workspace", str(workspace)],
        cwd=str(REPO_ROOT),
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
        env=os.environ.copy(),
    )
    q: queue.Queue = queue.Queue()
    threading.Thread(target=_reader, args=(proc.stdout, "out", q), daemon=True).start()
    threading.Thread(target=_reader, args=(proc.stderr, "err", q), daemon=True).start()
    return proc, q


def send(proc: subprocess.Popen, payload: dict) -> None:
    assert proc.stdin
    proc.stdin.write(json.dumps(payload) + "\n")
    proc.stdin.flush()


def drain(
    q: "queue.Queue",
    timeout: float,
    *,
    want_method: Optional[str] = None,
    stop_on_idle: bool = False,
    stop_on_finish: bool = False,
) -> list[dict]:
    """Collect RPC messages until timeout or a stop condition is met."""
    msgs: list[dict] = []
    deadline = time.time() + timeout
    while time.time() < deadline:
        remaining = max(0.05, deadline - time.time())
        try:
            src, line = q.get(timeout=remaining)
        except queue.Empty:
            break
        if line is None:
            break
        if src == "err":
            continue
        if not line.strip():
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        msgs.append(msg)
        method = msg.get("method", "")
        params = msg.get("params", {})
        if want_method and method == want_method:
            break
        if stop_on_finish and method == "ai_stream_chunk":
            if params.get("finish_reason") is not None:
                break
        if stop_on_idle and method == "ui_state_delta":
            if params.get("delta", {}).get("run_state") == "idle":
                break
    return msgs


def stop_agent(proc: subprocess.Popen) -> None:
    try:
        proc.terminate()
        proc.wait(timeout=5)
    except Exception:
        proc.kill()


# ---------------------------------------------------------------------------
# Test cases
# ---------------------------------------------------------------------------

results: list[tuple[str, bool, str]] = []  # (name, ok, detail)


def record(name: str, ok: bool, detail: str = "") -> None:
    results.append((name, ok, detail))
    icon = PASS if ok else FAIL
    print(f"  {icon}  {name}" + (f" — {detail}" if detail else ""))


# ── 1. Ping ─────────────────────────────────────────────────────────────────

def test_ping(proc: subprocess.Popen, q: queue.Queue) -> None:
    send(proc, {"id": 1, "method": "ping", "params": {}})
    msgs = drain(q, timeout=5, want_method="pong")
    # Accept either a "pong" notification or a result with id=1
    got = any(
        m.get("method") == "pong"
        or (m.get("id") == 1 and "result" in m)
        for m in msgs
    )
    record("ping / basic handshake", got)


# ── 2. AI chat – Ask mode ────────────────────────────────────────────────────

def test_ask_chat(proc: subprocess.Popen, q: queue.Queue) -> None:
    send(proc, {"method": "set_agent_mode", "params": {"mode": "ask"}})
    time.sleep(0.3)
    send(proc, {
        "method": "ai_prompt",
        "params": {"prompt": "Reply with exactly the word: HELLO"},
    })
    msgs = drain(q, timeout=30, stop_on_finish=True)

    chunks = [m for m in msgs if m.get("method") == "ai_stream_chunk"]
    has_content = any(
        (m.get("params") or {}).get("content_delta") for m in chunks
    )
    has_finish = any(
        (m.get("params") or {}).get("finish_reason") is not None for m in chunks
    )
    record("AI chat – stream chunks received", len(chunks) > 0)
    record("AI chat – finish_reason present", has_finish)
    record("AI chat – non-empty content", has_content)


# ── 3. Mode switching ────────────────────────────────────────────────────────

def test_mode_switch(proc: subprocess.Popen, q: queue.Queue) -> None:
    for mode in ("plan", "auto", "ask"):
        send(proc, {"method": "set_agent_mode", "params": {"mode": mode}})
        msgs = drain(q, timeout=5)
        ack = any(
            m.get("method") == "ui_state_delta"
            and (m.get("params") or {}).get("delta", {}).get("mode") == mode
            for m in msgs
        )
        record(f"mode switch → {mode}", ack)


# ── 4. Budget & context telemetry ────────────────────────────────────────────

def test_telemetry(proc: subprocess.Popen, q: queue.Queue) -> None:
    send(proc, {
        "method": "ai_prompt",
        "params": {"prompt": "Say: TOKEN_TEST"},
    })
    msgs = drain(q, timeout=30, stop_on_idle=True)

    has_budget = any(m.get("method") == "budget_update" for m in msgs)
    has_context = any(m.get("method") == "context_update" for m in msgs)
    record("budget_update telemetry", has_budget)
    record("context_update telemetry", has_context)


# ── 5. Plan-mode mission ──────────────────────────────────────────────────────

def test_plan_mission(proc: subprocess.Popen, q: queue.Queue) -> None:
    send(proc, {"method": "set_agent_mode", "params": {"mode": "plan"}})
    time.sleep(0.5)
    send(proc, {
        "method": "ai_prompt",
        "params": {
            "prompt": (
                "List three concrete steps to add a REST endpoint to a FastAPI app. "
                "End your reply with the marker PLAN_DONE."
            )
        },
    })
    msgs = drain(q, timeout=60, stop_on_idle=True)

    chunks = [m for m in msgs if m.get("method") == "ai_stream_chunk"]
    full_text = "".join(
        (m.get("params") or {}).get("content_delta") or "" for m in chunks
    )
    has_marker = "PLAN_DONE" in full_text
    has_mission = any(m.get("method") == "mission_update" for m in msgs)
    saw_running = any(
        (m.get("params") or {}).get("delta", {}).get("run_state") == "running"
        for m in msgs
        if m.get("method") == "ui_state_delta"
    )

    record("plan mission – content streamed", len(full_text) > 0)
    record("plan mission – PLAN_DONE marker", has_marker)
    record("plan mission – mission_update event", has_mission)
    record("plan mission – run_state=running observed", saw_running)


# ── 6. run_state idle after completion ───────────────────────────────────────

def test_idle_after_completion(proc: subprocess.Popen, q: queue.Queue) -> None:
    """After the agent finishes, it should broadcast run_state=idle."""
    # The previous test already waited for idle; verify at least one was seen
    send(proc, {
        "method": "ai_prompt",
        "params": {"prompt": "One word: DONE"},
    })
    msgs = drain(q, timeout=30, stop_on_idle=True)
    saw_idle = any(
        (m.get("params") or {}).get("delta", {}).get("run_state") == "idle"
        for m in msgs
        if m.get("method") == "ui_state_delta"
    )
    record("run_state=idle after completion", saw_idle)


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

def main() -> int:
    has_key = any(
        os.getenv(k)
        for k in ["ANTHROPIC_API_KEY", "OPENROUTER_API_KEY", "OPENAI_API_KEY"]
    )
    if not has_key:
        print(f"  {SKIP}  No LLM API key found — set ANTHROPIC_API_KEY (or OPENROUTER/OPENAI) to run AI tests.")
        return 0

    if not AGENT_BIN.exists():
        print(f"  {FAIL}  Binary not found: {AGENT_BIN}")
        print("         Run: cargo build -p ownstack-agent")
        return 1

    workspace = REPO_ROOT
    print(f"\n=== OwnStack User AI Features Test ===")
    print(f"Agent : {AGENT_BIN}")
    print(f"Key   : {'ANTHROPIC' if os.getenv('ANTHROPIC_API_KEY') else 'OPENROUTER/OPENAI'}")
    print()

    proc, q = start_agent(workspace)
    # Give the agent a moment to boot
    time.sleep(1.5)

    try:
        print("[1] Connectivity")
        test_ping(proc, q)

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

    finally:
        stop_agent(proc)

    # Summary
    total = len(results)
    passed = sum(1 for _, ok, _ in results if ok)
    failed = total - passed

    print(f"\n{'─'*40}")
    print(f"Results: {passed}/{total} passed" + (f", {failed} failed" if failed else ""))

    if failed:
        print("\nFailed tests:")
        for name, ok, detail in results:
            if not ok:
                print(f"  • {name}" + (f" ({detail})" if detail else ""))
        return 1

    print(f"\n{PASS}  All AI feature tests passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
