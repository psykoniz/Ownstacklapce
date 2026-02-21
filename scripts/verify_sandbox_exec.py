
import subprocess
import json
import time
import os
import sys
import threading

def send_rpc(proc, method, params):
    msg = {
        "jsonrpc": "2.0",
        "method": method,
        "params": params
    }
    encoded = json.dumps(msg) + "\n"
    proc.stdin.write(encoded.encode('utf-8'))
    proc.stdin.flush()
    print(f"Sent: {method}")

def kill_process_by_name(name):
    subprocess.run(f"taskkill /F /IM {name}", shell=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

def read_stdout(proc, stop_event, result_container):
    print("Stdout thread started...")
    for line in iter(proc.stdout.readline, b''):
        if stop_event.is_set():
            break
        line_str = line.decode('utf-8', errors='ignore').strip()
        if not line_str:
            continue
        print(f"Received: {line_str}")
        try:
            rpc = json.loads(line_str)
            if rpc.get("method") == "own_stack":
                inner = rpc.get("params", {}).get("message", {})
                if inner.get("method") == "tool_result_msg":
                    json_res = inner.get("params", {}).get("json_result", "")
                    print(f"Tool Result RAW: {json_res}")
                    tool_res = json.loads(json_res)
                    result_container['output'] = tool_res
                    stop_event.set()
        except json.JSONDecodeError:
            pass

def read_stderr(proc):
    print("Stderr thread started...")
    for line in iter(proc.stderr.readline, b''):
        line_str = line.decode('utf-8', errors='ignore').strip()
        if line_str:
            print(f"PROXY-STDERR: {line_str}")

def main():
    print("Starting Sandbox Execution Verification...")
    
    kill_process_by_name("lapce-proxy.exe")
    kill_process_by_name("ownstack-agent.exe")
    time.sleep(1)

    proxy_path = r"target\debug\lapce-proxy.exe"
    proc = subprocess.Popen(
        [proxy_path, "--proxy"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        cwd=os.getcwd()
    )
    
    stop_event = threading.Event()
    result_container = {}
    
    stdout_reader = threading.Thread(target=read_stdout, args=(proc, stop_event, result_container))
    stdout_reader.daemon = True
    stdout_reader.start()
    
    stderr_reader = threading.Thread(target=read_stderr, args=(proc,))
    stderr_reader.daemon = True
    stderr_reader.start()
    
    init_params = {
        "workspace": os.getcwd(),
        "disabled_volts": [],
        "extra_plugin_paths": [],
        "plugin_configurations": {},
        "window_id": 1,
        "tab_id": 1
    }
    send_rpc(proc, "initialize", init_params)
    time.sleep(1)
    
    # USING cmd /c echo hello
    tool_exec_params = {
        "message": {
            "method": "tool_exec",
            "params": {
                "tool_name": "exec",
                "command": "cmd /c echo hello"
            }
        }
    }
    send_rpc(proc, "own_stack", tool_exec_params)
    
    print("Sent ToolExec. Waiting for result...")
    
    waiting = 0
    while waiting < 15 and not stop_event.is_set():
        time.sleep(1)
        waiting += 1
        
    if "output" in result_container:
        output = result_container["output"]
        # In ownstack-agent, the successful stdout is stored in the 'output' field of ToolResult struct
        stdout_content = output.get("stdout", "").strip()
        print(f"Final STDOUT: '{stdout_content}'")
        
        if "hello" in stdout_content:
            print("SUCCESS: Sandbox executed command and returned expected output.")
            kill_process_by_name("lapce-proxy.exe")
            kill_process_by_name("ownstack-agent.exe")
            sys.exit(0)
        else:
            print("FAILURE: Output verification failed.")
            print(f"Expected 'hello', got: '{stdout_content}'")
            sys.exit(1)
    else:
        print("FAILURE: Timeout waiting for ToolResultMsg.")
        kill_process_by_name("lapce-proxy.exe")
        kill_process_by_name("ownstack-agent.exe")
        sys.exit(1)

if __name__ == "__main__":
    main()
