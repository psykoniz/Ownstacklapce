#!/usr/bin/env python3
"""
Policy approval smoke test for OwnStack agent.

Flow:
1) Start ownstack-agent in IDE RPC mode.
2) Send ToolExec(exec: "git push origin main") to force Ask policy.
3) Wait for PolicyPrompt from agent.
4) Send PolicyResponse(approved=true|false).
5) Validate ToolResultMsg is received.
"""

from __future__ import annotations

import argparse
import json
import os
import queue
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any, Optional


def start_agent(repo_root: Path, workspace: Path, agent_bin: Optional[Path]) -> subprocess.Popen[str]:
    if agent_bin is not None:
        cmd = [str(agent_bin), "--workspace", str(workspace)]
    else:
        # Fallback that works even if the binary is not pre-built.
        cmd = [
            "cargo",
            "run",
            "-p",
            "ownstack-agent",
            "--bin",
            "ownstack-agent",
            "--",
            "--workspace",
            str(workspace),
        ]

    return subprocess.Popen(
        cmd,
        cwd=str(repo_root),
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )


def spawn_stdout_reader(proc: subprocess.Popen[str], out_queue: "queue.Queue[str]") -> threading.Thread:
    def _reader() -> None:
        assert proc.stdout is not None
        for line in proc.stdout:
            out_queue.put(line.rstrip("\n"))

    thread = threading.Thread(target=_reader, daemon=True)
    thread.start()
    return thread


def send_rpc(proc: subprocess.Popen[str], payload: dict[str, Any]) -> None:
    assert proc.stdin is not None
    proc.stdin.write(json.dumps(payload) + "\n")
    proc.stdin.flush()


def read_rpc(out_queue: "queue.Queue[str]", timeout_s: float) -> Optional[dict[str, Any]]:
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        remaining = max(0.0, deadline - time.time())
        try:
            line = out_queue.get(timeout=remaining)
        except queue.Empty:
            return None
        if not line.strip():
            continue
        try:
            return json.loads(line)
        except json.JSONDecodeError:
            # Ignore non-JSON lines.
            continue
    return None


def main() -> int:
    parser = argparse.ArgumentParser(description="Policy approval E2E smoke test")
    parser.add_argument("--workspace", default=".", help="Workspace path")
    parser.add_argument("--repo-root", default=".", help="Repository root path")
    parser.add_argument(
        "--agent-bin",
        default="",
        help="Path to ownstack-agent binary (optional). If omitted, cargo run is used.",
    )
    parser.add_argument(
        "--approve",
        action="store_true",
        help="Approve the policy prompt instead of denying it.",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=45.0,
        help="Max wait per stage in seconds",
    )
    parser.add_argument(
        "--command",
        default="docker rm __ownstack_missing_container__",
        help="Command sent through ToolExec to trigger policy approval",
    )
    args = parser.parse_args()

    repo_root = Path(args.repo_root).resolve()
    workspace = Path(args.workspace).resolve()
    agent_bin = Path(args.agent_bin).resolve() if args.agent_bin else None

    proc = start_agent(repo_root=repo_root, workspace=workspace, agent_bin=agent_bin)
    out_queue: "queue.Queue[str]" = queue.Queue()
    reader_thread = spawn_stdout_reader(proc, out_queue)
    _ = reader_thread

    try:
        # Trigger Ask policy path.
        send_rpc(
            proc,
            {
                "method": "tool_exec",
                "params": {
                    "command": args.command,
                    "tool_name": "exec",
                },
            },
        )

        policy_prompt = None
        started = time.time()
        while time.time() - started < args.timeout:
            msg = read_rpc(out_queue, timeout_s=1.0)
            if msg is None:
                continue
            if msg.get("method") == "policy_prompt":
                policy_prompt = msg
                break

        if policy_prompt is None:
            print("FAIL: policy_prompt not received", file=sys.stderr)
            return 1

        params = policy_prompt.get("params", {})
        print(f"policy_prompt_command={params.get('command', '')}")
        print(f"policy_prompt_reason={params.get('reason', '')}")

        send_rpc(
            proc,
            {
                "method": "policy_response",
                "params": {"approved": bool(args.approve)},
            },
        )

        tool_result_msg = None
        started = time.time()
        while time.time() - started < args.timeout:
            msg = read_rpc(out_queue, timeout_s=1.0)
            if msg is None:
                continue
            if msg.get("method") == "tool_result_msg":
                tool_result_msg = msg
                break

        if tool_result_msg is None:
            print("FAIL: tool_result_msg not received", file=sys.stderr)
            return 1

        raw = tool_result_msg.get("params", {}).get("json_result", "{}")
        result = json.loads(raw)
        success = bool(result.get("success", False))
        stderr = str(result.get("stderr", ""))
        print(f"tool_result_success={success}")
        print(f"tool_result_stderr={stderr}")

        if not args.approve:
            if success:
                print("FAIL: deny path unexpectedly succeeded", file=sys.stderr)
                return 1
            if "denied" not in stderr.lower() and "approval" not in stderr.lower():
                print(
                    "WARN: deny path failed as expected but stderr does not mention denial",
                    file=sys.stderr,
                )

        print("PASS")
        return 0
    finally:
        try:
            proc.terminate()
            proc.wait(timeout=5)
        except Exception:
            proc.kill()


if __name__ == "__main__":
    raise SystemExit(main())
