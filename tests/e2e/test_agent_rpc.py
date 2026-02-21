#!/usr/bin/env python3
import json
import os
import queue
import subprocess
import sys
import threading
from pathlib import Path
import time


def _pipe_reader(pipe, out_queue: queue.Queue, source: str) -> None:
    try:
        for line in iter(pipe.readline, ""):
            out_queue.put((source, line.rstrip("\n")))
    finally:
        out_queue.put((source, None))


def test_agent_rpc() -> bool:
    print("--- Testing Agent RPC Handshake ---")

    is_windows = os.name == "nt"
    agent_bin = Path("target/debug/ownstack-agent.exe") if is_windows else Path("target/debug/ownstack-agent")

    if not agent_bin.exists():
        print(f"Error: {agent_bin} not found")
        return False

    proc = subprocess.Popen(
        [str(agent_bin), "--workspace", "."],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )

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
        "params": {"prompt": "Respond with exactly: pong"},
    }
    proc.stdin.write(json.dumps(prompt_rpc) + "\n")
    proc.stdin.flush()

    deadline = time.time() + 20
    seen_stream = False
    seen_finish = False

    while time.time() < deadline:
        try:
            source, line = events.get(timeout=0.25)
        except queue.Empty:
            continue

        if line is None:
            continue

        if source == "stderr":
            continue

        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue

        if msg.get("method") == "ai_stream_chunk":
            seen_stream = True
            if msg.get("params", {}).get("finish_reason") is not None:
                seen_finish = True
                break

    try:
        proc.terminate()
        proc.wait(timeout=5)
    except Exception:
        proc.kill()

    ok = seen_stream and seen_finish
    print("PASS" if ok else "FAIL")
    return ok


if __name__ == "__main__":
    raise SystemExit(0 if test_agent_rpc() else 1)
