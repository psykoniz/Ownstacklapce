#!/usr/bin/env python3
"""
Complex E2E mission test for OwnStack agent-first runtime.

Flow:
1) Start ownstack-agent in RPC mode.
2) Set runtime mode to Plan via RPC.
3) Send a complex mission prompt to create a mini Rust project.
4) Validate runtime state telemetry (UiStateDelta + budget/context updates).
5) Validate generated files and run local `cargo test` in the generated project.
"""

from __future__ import annotations

import argparse
import json
import os
import queue
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any, Optional


IS_WINDOWS = os.name == "nt"
EXE_SUFFIX = ".exe" if IS_WINDOWS else ""


def resolve_agent_command(
    repo_root: Path,
    workspace: Path,
    agent_bin: Optional[Path],
) -> list[str]:
    if agent_bin is not None:
        return [str(agent_bin), "--workspace", str(workspace)]

    candidate = repo_root / f"target/debug/ownstack-agent{EXE_SUFFIX}"
    if candidate.exists():
        return [str(candidate), "--workspace", str(workspace)]

    return [
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


def start_agent(
    repo_root: Path,
    workspace: Path,
    agent_bin: Optional[Path],
    env: dict[str, str],
) -> subprocess.Popen[str]:
    cmd = resolve_agent_command(repo_root, workspace, agent_bin)
    return subprocess.Popen(
        cmd,
        cwd=str(repo_root),
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
        env=env,
    )


def spawn_reader(
    pipe: Any,
    source: str,
    out_queue: "queue.Queue[tuple[str, Optional[str]]]",
) -> threading.Thread:
    def _reader() -> None:
        try:
            for line in pipe:
                out_queue.put((source, line.rstrip("\n")))
        finally:
            out_queue.put((source, None))

    t = threading.Thread(target=_reader, daemon=True)
    t.start()
    return t


def send_rpc(proc: subprocess.Popen[str], payload: dict[str, Any]) -> None:
    assert proc.stdin is not None
    proc.stdin.write(json.dumps(payload) + "\n")
    proc.stdin.flush()


def read_rpc(
    out_queue: "queue.Queue[tuple[str, Optional[str]]]",
    timeout_s: float,
) -> Optional[dict[str, Any]]:
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        remaining = max(0.0, deadline - time.time())
        try:
            source, line = out_queue.get(timeout=remaining)
        except queue.Empty:
            return None

        if line is None:
            continue
        if source == "stderr":
            # Keep stderr noise out of JSON parser.
            continue
        if not line.strip():
            continue

        try:
            return json.loads(line)
        except json.JSONDecodeError:
            continue
    return None


def mission_prompt(project_rel: str) -> str:
    return f"""
Create a minimal Rust CLI project under "{project_rel}".

Requirements:
1. Create these files:
   - {project_rel}/Cargo.toml
   - {project_rel}/src/main.rs
   - {project_rel}/src/lib.rs
   - {project_rel}/tests/smoke.rs
   - {project_rel}/README.md
2. `src/lib.rs` must define:
   - `pub fn add(a: i32, b: i32) -> i32`
3. `tests/smoke.rs` must test that `add(2, 3) == 5`.
4. `README.md` must contain exactly this phrase:
   - OwnStack E2E mini project
5. Run `cargo test` inside "{project_rel}" using tool execution.
6. At the very end, answer with this exact marker on one line:
   - MISSION_OK:{project_rel}

Important:
- Stay strictly inside the current workspace.
- If a file exists, overwrite it safely.
""".strip()


def validate_project(workspace: Path, project_rel: str) -> tuple[bool, str]:
    project_dir = workspace / project_rel
    required = [
        project_dir / "Cargo.toml",
        project_dir / "src" / "main.rs",
        project_dir / "src" / "lib.rs",
        project_dir / "tests" / "smoke.rs",
        project_dir / "README.md",
    ]

    missing = [str(p) for p in required if not p.exists()]
    if missing:
        return False, f"Missing files: {missing}"

    readme = (project_dir / "README.md").read_text(encoding="utf-8", errors="ignore")
    if "OwnStack E2E mini project" not in readme:
        return False, "README marker missing"

    result = subprocess.run(
        ["cargo", "test"],
        cwd=str(project_dir),
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        return False, f"Local cargo test failed:\n{result.stdout}\n{result.stderr}"

    return True, "Project validated"


def main() -> int:
    parser = argparse.ArgumentParser(description="OwnStack complex mission E2E")
    parser.add_argument("--repo-root", default=".", help="Repository root")
    parser.add_argument("--workspace", default=".", help="Workspace path")
    parser.add_argument("--agent-bin", default="", help="Path to ownstack-agent binary")
    parser.add_argument("--project-rel", default=".ownstack_e2e/mini_project_runtime")
    parser.add_argument("--timeout", type=float, default=180.0)
    args = parser.parse_args()

    has_keys = any(
        os.getenv(k)
        for k in ["OPENROUTER_API_KEY", "ANTHROPIC_API_KEY", "OPENAI_API_KEY"]
    )
    if not has_keys:
        print("SKIP: no LLM API key found in environment.")
        return 0

    repo_root = Path(args.repo_root).resolve()
    workspace = Path(args.workspace).resolve()
    agent_bin = Path(args.agent_bin).resolve() if args.agent_bin else None

    project_dir = workspace / args.project_rel
    if project_dir.exists():
        shutil.rmtree(project_dir)

    env = os.environ.copy()
    proc = start_agent(repo_root, workspace, agent_bin, env)

    q: "queue.Queue[tuple[str, Optional[str]]]" = queue.Queue()
    t_out = spawn_reader(proc.stdout, "stdout", q)
    t_err = spawn_reader(proc.stderr, "stderr", q)
    _ = (t_out, t_err)

    stream_chunks = 0
    mission_updates = 0
    budget_updates = 0
    context_updates = 0
    ui_deltas = 0
    saw_running = False
    saw_idle = False
    saw_plan_mode = False
    saw_finish = False
    full_content = ""

    try:
        # Set runtime mode to Plan before prompting.
        send_rpc(
            proc,
            {"method": "set_agent_mode", "params": {"mode": "plan"}},
        )

        # Wait briefly for mode ack delta.
        mode_deadline = time.time() + 10
        while time.time() < mode_deadline:
            msg = read_rpc(q, timeout_s=0.5)
            if msg is None:
                continue
            method = msg.get("method")
            params = msg.get("params", {})
            if method == "ui_state_delta":
                ui_deltas += 1
                delta = params.get("delta", {})
                if delta.get("mode") == "plan":
                    saw_plan_mode = True
                run_state = delta.get("run_state")
                if run_state == "running":
                    saw_running = True
                if run_state == "idle":
                    saw_idle = True

        send_rpc(
            proc,
            {
                "method": "ai_prompt",
                "params": {"prompt": mission_prompt(args.project_rel)},
            },
        )

        deadline = time.time() + args.timeout
        while time.time() < deadline and not saw_finish:
            msg = read_rpc(q, timeout_s=1.0)
            if msg is None:
                continue
            method = msg.get("method")
            params = msg.get("params", {})

            if method == "ai_stream_chunk":
                delta = params.get("content_delta") or ""
                if delta:
                    stream_chunks += 1
                    full_content += delta
                if params.get("finish_reason") is not None:
                    saw_finish = True
            elif method == "mission_update":
                mission_updates += 1
            elif method == "budget_update":
                budget_updates += 1
            elif method == "context_update":
                context_updates += 1
            elif method == "ui_state_delta":
                ui_deltas += 1
                delta = params.get("delta", {})
                if delta.get("mode") == "plan":
                    saw_plan_mode = True
                run_state = delta.get("run_state")
                if run_state == "running":
                    saw_running = True
                if run_state == "idle":
                    saw_idle = True

        print(f"stream_chunks={stream_chunks}")
        print(f"mission_updates={mission_updates}")
        print(f"ui_deltas={ui_deltas}")
        print(f"budget_updates={budget_updates}")
        print(f"context_updates={context_updates}")
        print(f"saw_plan_mode={saw_plan_mode}")
        print(f"saw_running={saw_running}")
        print(f"saw_idle={saw_idle}")
        print(f"saw_finish={saw_finish}")

        if not saw_finish:
            print("FAIL: stream did not finish in time", file=sys.stderr)
            return 1
        if "MISSION_OK:" not in full_content:
            print("FAIL: completion marker not found in final content", file=sys.stderr)
            return 1
        if not saw_plan_mode:
            print("FAIL: no runtime mode ack for plan", file=sys.stderr)
            return 1
        if not (saw_running and saw_idle):
            print("FAIL: run_state transitions running->idle not observed", file=sys.stderr)
            return 1
        if budget_updates == 0 or context_updates == 0:
            print("FAIL: budget/context runtime telemetry not observed", file=sys.stderr)
            return 1

        ok, detail = validate_project(workspace, args.project_rel)
        if not ok:
            print(f"FAIL: {detail}", file=sys.stderr)
            return 1

        print("PASS: complex mission E2E validated")
        return 0
    finally:
        try:
            proc.terminate()
            proc.wait(timeout=5)
        except Exception:
            proc.kill()


if __name__ == "__main__":
    raise SystemExit(main())
