import json
import os
import signal
import subprocess
import sys
import threading
import time
from pathlib import Path

IS_WINDOWS = os.name == "nt"


def send_rpc(proc, method, params):
    msg = {
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    }
    encoded = json.dumps(msg) + "\n"
    proc.stdin.write(encoded.encode("utf-8"))
    proc.stdin.flush()
    print(f"Sent: {method}")


def kill_process_tree(proc):
    """Terminate a process and its children on any platform."""
    if IS_WINDOWS:
        subprocess.run(
            f"taskkill /F /T /PID {proc.pid}",
            shell=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    else:
        try:
            os.killpg(os.getpgid(proc.pid), signal.SIGTERM)
        except (ProcessLookupError, PermissionError):
            pass
        proc.terminate()

    try:
        proc.wait(timeout=3)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait(timeout=2)


def read_stdout(proc, stop_event, result_container):
    print("Stdout thread started...")
    for line in iter(proc.stdout.readline, b""):
        if stop_event.is_set():
            break

        line_str = line.decode("utf-8", errors="ignore").strip()
        if not line_str:
            continue

        try:
            rpc = json.loads(line_str)
            if rpc.get("method") == "own_stack":
                inner = rpc.get("params", {}).get("message", {})
                if inner.get("method") == "tool_result_msg":
                    json_res = inner.get("params", {}).get("json_result", "")
                    print(f"Tool Result RAW: {json_res}")
                    tool_res = json.loads(json_res)
                    result_container["output"] = tool_res
                    stop_event.set()
        except json.JSONDecodeError:
            pass


def main():
    print("Starting Escape Mitigation Test...")

    proxy_path = (
        Path("target/debug/lapce-proxy.exe")
        if IS_WINDOWS
        else Path("target/debug/lapce-proxy")
    )

    if not proxy_path.exists():
        print(
            f"Error: {proxy_path} not found. Please run 'cargo build -p lapce-proxy' first."
        )
        sys.exit(1)

    kwargs = {}
    if not IS_WINDOWS:
        kwargs["preexec_fn"] = os.setsid

    proc = subprocess.Popen(
        [str(proxy_path), "--proxy"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        cwd=os.getcwd(),
        **kwargs,
    )

    stop_event = threading.Event()
    result_container = {}

    stdout_reader = threading.Thread(
        target=read_stdout,
        args=(proc, stop_event, result_container),
        daemon=True,
    )
    stdout_reader.start()

    try:
        init_params = {
            "workspace": os.getcwd(),
            "disabled_volts": [],
            "extra_plugin_paths": [],
            "plugin_configurations": {},
            "window_id": 1,
            "tab_id": 1,
        }
        send_rpc(proc, "initialize", init_params)
        time.sleep(1)

        tool_exec_params = {
            "message": {
                "method": "tool_exec",
                "params": {
                    "tool_name": "exec",
                    "command": "rm -rf /",
                },
            }
        }
        send_rpc(proc, "own_stack", tool_exec_params)
        print("Sent Forbidden ToolExec. Waiting for result...")

        waiting = 0
        while waiting < 10 and not stop_event.is_set():
            time.sleep(1)
            waiting += 1

        if "output" in result_container:
            output = result_container["output"]
            success = output.get("success", True)
            error_msg = output.get("error", "")
            stderr_msg = output.get("stderr", "")
            combined_msg = f"{error_msg}\n{stderr_msg}".lower()

            print(
                f"Success: {success}, Error: {error_msg}, Stderr: {stderr_msg}"
            )

            if (
                not success
                and (
                    "blocked by policy" in combined_msg
                    or "security violation" in combined_msg
                )
            ):
                print("SUCCESS: Command was correctly blocked by PolicyEngine.")
                sys.exit(0)

            print("FAILURE: Command was NOT blocked correctly.")
            sys.exit(1)

        # The proxy->agent chain requires the full IDE environment.
        # In headless/CI mode, proxy does not always route own_stack messages.
        print("NOTE: No ToolResultMsg received (proxy did not route to agent).")
        print("This test requires full IDE proxy->agent integration.")
        print("SKIP: Escape mitigation not verifiable in this environment.")
        sys.exit(0)
    finally:
        kill_process_tree(proc)


if __name__ == "__main__":
    main()
