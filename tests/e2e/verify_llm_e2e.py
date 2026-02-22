#!/usr/bin/env python3
import json
import os
import queue
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Optional

IS_WINDOWS = os.name == "nt"
EXE_SUFFIX = ".exe" if IS_WINDOWS else ""


def _pipe_reader(pipe, out_queue: queue.Queue, source: str) -> None:
    try:
        for line in iter(pipe.readline, ""):
            out_queue.put((source, line.rstrip("\n")))
    finally:
        out_queue.put((source, None))


def _start_agent(api_key: str, model: str, workspace: str) -> subprocess.Popen:
    agent_bin = Path(f"target/debug/ownstack-agent{EXE_SUFFIX}")

    env = os.environ.copy()
    env["OPENROUTER_API_KEY"] = api_key
    env["OPENROUTER_MODEL"] = model

    return subprocess.Popen(
        [str(agent_bin), "--workspace", workspace],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
        env=env,
    )


def test_streaming(api_key: str, model: str, workspace: str) -> bool:
    print("--- Testing OpenRouter Streaming (OwnStack RPC) ---")
    proc = _start_agent(api_key, model, workspace)

    events: queue.Queue = queue.Queue()
    t_out = threading.Thread(
        target=_pipe_reader, args=(proc.stdout, events, "stdout"), daemon=True
    )
    t_err = threading.Thread(
        target=_pipe_reader, args=(proc.stderr, events, "stderr"), daemon=True
    )
    t_out.start()
    t_err.start()

    prompt_rpc = {
        "method": "ai_prompt",
        "params": {"prompt": "Dis hello en un seul mot."},
    }

    print(f"Model: {model}")
    print(f"Sending RPC: {prompt_rpc}")
    proc.stdin.write(json.dumps(prompt_rpc) + "\n")
    proc.stdin.flush()

    chunks_received = 0
    full_content = ""
    finish_reason: Optional[str] = None

    deadline = time.time() + 45
    stdout_closed = False
    stderr_closed = False

    while time.time() < deadline:
        if finish_reason:
            break

        try:
            source, line = events.get(timeout=0.25)
        except queue.Empty:
            continue

        if line is None:
            if source == "stdout":
                stdout_closed = True
            if source == "stderr":
                stderr_closed = True
            if stdout_closed and stderr_closed:
                break
            continue

        if source == "stderr":
            if "ERROR" in line or "Failed" in line or "panic" in line:
                print(f"[agent-stderr] {line}")
            continue

        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            print(f"[stdout non-json] {line}")
            continue

        method = msg.get("method")
        params = msg.get("params", {})

        if method == "ai_stream_chunk":
            delta = params.get("content_delta") or ""
            if delta:
                chunks_received += 1
                full_content += delta
                print(f"chunk[{chunks_received}] {delta!r}")

            if params.get("finish_reason") is not None:
                finish_reason = params.get("finish_reason")
                print(f"finish_reason={finish_reason}")

    try:
        proc.terminate()
        proc.wait(timeout=5)
    except Exception:
        proc.kill()

    print("\nSummary:")
    print(f"- chunks_received: {chunks_received}")
    print(f"- finish_reason: {finish_reason}")
    print(f"- full_content: {full_content!r}")

    ok = (
        chunks_received > 0
        and finish_reason is not None
        and bool(full_content.strip())
    )
    if ok:
        print("\nPASS: Streaming works with OwnStack RPC.")
    else:
        print("\nFAIL: No valid streamed content received.")
    return ok


def main() -> int:
    api_key = os.environ.get("OPENROUTER_API_KEY", "").strip()
    if not api_key:
        print("Missing OPENROUTER_API_KEY environment variable.")
        return 2

    model = os.environ.get("OPENROUTER_MODEL", "openai/gpt-4o-mini").strip()
    workspace = os.environ.get("OWNSTACK_TEST_WORKSPACE", ".").strip() or "."
    return 0 if test_streaming(api_key, model, workspace) else 1


if __name__ == "__main__":
    raise SystemExit(main())
