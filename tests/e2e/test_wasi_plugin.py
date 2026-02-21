#!/usr/bin/env python3
import json
import os
import queue
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import (
    Ed25519PrivateKey,
)

def _pipe_reader(pipe, out_queue: queue.Queue, source: str) -> None:
    try:
        for line in iter(pipe.readline, ""):
            out_queue.put((source, line.rstrip("\n")))
    finally:
        out_queue.put((source, None))

def _run_checked(cmd, cwd=None):
    proc = subprocess.run(
        cmd,
        cwd=cwd,
        text=True,
        capture_output=True,
    )
    if proc.returncode != 0:
        print(f"[cmd failed] {' '.join(cmd)}")
        if proc.stdout.strip():
            print(proc.stdout)
        if proc.stderr.strip():
            print(proc.stderr)
        return False
    return True

def _ensure_agent_binary() -> str:
    is_windows = os.name == "nt"
    agent_bin = (
        Path("target/debug/ownstack-agent.exe")
        if is_windows
        else Path("target/debug/ownstack-agent")
    )
    if not _run_checked(
        ["cargo", "build", "-p", "ownstack-agent", "--bin", "ownstack-agent"]
    ):
        raise RuntimeError("Failed to build ownstack-agent binary")
    if not agent_bin.exists():
        raise RuntimeError(
            f"Expected agent binary missing: {agent_bin}. "
            "Aborting to avoid running stale target/debug/deps artifacts."
        )
    return str(agent_bin)

def _ensure_hello_world_wasm() -> Path:
    plugin_dir = Path("plugins/examples/hello_world")
    target_wasip1 = plugin_dir / "target/wasm32-wasip1/release/hello_world.wasm"
    target_wasi = plugin_dir / "target/wasm32-wasi/release/hello_world.wasm"

    build_ok = _run_checked(
        ["cargo", "build", "--release", "--target", "wasm32-wasip1"],
        cwd=str(plugin_dir),
    )
    if build_ok and target_wasip1.exists():
        built = target_wasip1
    else:
        build_ok = _run_checked(
            ["cargo", "build", "--release", "--target", "wasm32-wasi"],
            cwd=str(plugin_dir),
        )
        if not build_ok or not target_wasi.exists():
            raise RuntimeError(
                "Failed to build hello_world WASM plugin for wasm32-wasip1/wasm32-wasi"
            )
        built = target_wasi

    dest = Path("plugins/hello_world.wasm")
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(built, dest)
    return dest

def _sign_plugin_name(wasm_path: Path) -> str:
    plugin_name = wasm_path.stem.encode("utf-8")
    private_key = Ed25519PrivateKey.generate()
    public_key = private_key.public_key().public_bytes(
        encoding=serialization.Encoding.Raw,
        format=serialization.PublicFormat.Raw,
    )
    signature = private_key.sign(plugin_name)

    sig_path = wasm_path.with_suffix(".wasm.sig")
    sig_path.write_bytes(signature)
    return public_key.hex()

def test_wasi_execution():
    print("--- Testing WASI Plugin Execution (OwnStack RPC) ---")

    agent_bin = _ensure_agent_binary()
    wasm_path = _ensure_hello_world_wasm()
    trusted_pubkey_hex = _sign_plugin_name(wasm_path)
    print(f"Using agent binary: {agent_bin}")
    print(f"Using plugin wasm:  {wasm_path}")
    
    # Start the agent
    env = os.environ.copy()
    env["OWNSTACK_PLUGIN_TRUSTED_PUBLIC_KEY_HEX"] = trusted_pubkey_hex

    proc = subprocess.Popen(
        [agent_bin, "--workspace", "."],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
        env=env,
    )

    events: queue.Queue = queue.Queue()
    t_out = threading.Thread(target=_pipe_reader, args=(proc.stdout, events, "stdout"), daemon=True)
    t_err = threading.Thread(target=_pipe_reader, args=(proc.stderr, events, "stderr"), daemon=True)
    t_out.start()
    t_err.start()

    # RPC message to trigger tool execution (WASI plugin)
    # The plugin is named after its filename (without extension)
    tool_exec_rpc = {
        "method": "tool_exec",
        "params": {
            "tool_name": "hello_world",
            "command": json.dumps({"name": "AgenticAI"})
        }
    }

    print(f"Sending RPC: {tool_exec_rpc}")
    proc.stdin.write(json.dumps(tool_exec_rpc) + "\n")
    proc.stdin.flush()

    deadline = time.time() + 15
    result_received = False
    success = False

    while time.time() < deadline:
        try:
            source, line = events.get(timeout=0.25)
        except queue.Empty:
            continue

        if line is None:
            continue

        if source == "stderr":
            print(f"[agent-stderr] {line}")
            continue

        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue

        if msg.get("method") == "tool_result_msg":
            result_received = True
            raw_result = msg.get("params", {}).get("json_result", "{}")
            result = json.loads(raw_result)
            print(f"Received Result: {result}")
            if result.get("success") and "Hello, AgenticAI!" in result.get("stdout", ""):
                success = True
            break

    try:
        proc.terminate()
        proc.wait(timeout=5)
    except Exception:
        proc.kill()

    if success:
        print("\nPASS: WASI Plugin execution verified!")
    else:
        if result_received:
            print("\nFAIL: Tool result received but indicates failure.")
        else:
            print("\nFAIL: No tool_result_msg received before timeout.")
    return success

if __name__ == "__main__":
    if not test_wasi_execution():
        sys.exit(1)
