
import subprocess
import json
import time
import os
import sys

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

def main():
    print("Starting process verification...")
    
    # 1. Cleanup
    kill_process_by_name("lapce-proxy.exe")
    kill_process_by_name("ownstack-agent.exe")
    time.sleep(1)

    # 2. Spawn lapce-proxy
    proxy_path = r"target\debug\lapce-proxy.exe"
    if not os.path.exists(proxy_path):
        print(f"Error: {proxy_path} not found")
        sys.exit(1)
        
    print(f"Spawning {proxy_path}...")
    proc = subprocess.Popen(
        [proxy_path, "--proxy"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        cwd=os.getcwd()
    )
    
    # 3. Send Initialize
    init_params = {
        "workspace": os.getcwd(),
        "disabled_volts": [],
        "extra_plugin_paths": [],
        "plugin_configurations": {},
        "window_id": 1,
        "tab_id": 1
    }
    # Note: lapce-proxy might expect just the notification object, not wrapped in jsonrpc 2.0 with ID?
    # lapce-rpc uses a custom transport. Let's look at `read_msg` in `stdio.rs` later if this fails.
    # Usually it expects Content-Length header.
    
    # Actually, RpcMessage definition:
    # pub enum RpcMessage { Request, Response, Notification, Error }
    # Serialization usually doesn't include "jsonrpc": "2.0" in Rust serde unless explicitly added.
    # Looking at `lapce-rpc` again, it seems to handle `Notification` directly.
    # Let's try sending the Notification object directly (with headers).
    
    init_msg = {
        "method": "initialize",
        "params": init_params
    }
    # Wrap in RpcMessage::Notification structure if needed?
    # No, `ProxyMessage` is `RpcMessage<ProxyRequest, ProxyNotification, ProxyResponse>`
    # serde serialization of enum RpcMessage::Notification(val)
    # usually outputs {"method": "...", "params": ...} if untagged?
    # Wait, `RpcMessage` is:
    # enum RpcMessage { Request(id, req), Response(id, resp), Notification(notif), Error(...) }
    # This is an externally tagged enum usually?
    # Or maybe it uses custom serialization. 
    # Let's assume standard LSP-like JSON-RPC which lapce seems to confirm.
    # But for safety, I will try the standard format first.
    
    send_rpc(proc, "initialize", init_params)
    time.sleep(1)
    
    # 4. Same for OpenPaths to ensure it's alive
    # send_rpc(proc, "open_paths", {"paths": []})
    
    # 5. Send OwnStack trigger
    trigger_params = {
        "message": {
            "method": "ai_prompt",
            "params": {
                "prompt": "Trigger Agent Spawn"
            }
        }
    }
    send_rpc(proc, "own_stack", trigger_params)
    
    print("Sent OwnStack trigger. Waiting 3 seconds for spawn...")
    time.sleep(3)
    
    # 6. Verify Child Process
    # wmic process where (ParentProcessId={pid}) get Commandline
    cmd = f"wmic process where (ParentProcessId={proc.pid}) get Caption"
    result = subprocess.run(cmd, shell=True, capture_output=True, text=True)
    
    print("Child processes:")
    print(result.stdout)
    
    if "ownstack-agent.exe" in result.stdout:
        print("SUCCESS: ownstack-agent.exe successfully spawned by lapce-proxy.")
        # proc.terminate()
        kill_process_by_name("lapce-proxy.exe")
        kill_process_by_name("ownstack-agent.exe")
        sys.exit(0)
    else:
        print("FAILURE: ownstack-agent.exe not found as child.")
        print(f"Proxy stderr: {proc.stderr.read().decode('utf-8', errors='ignore')}")
        kill_process_by_name("lapce-proxy.exe")
        sys.exit(1)

if __name__ == "__main__":
    main()
