import subprocess
import json
import os
import sys
import tempfile
from pathlib import Path

# Mock MCP Server
MOCK_SERVER_CODE = """
import sys
import json

def log(msg):
    with open("mcp_mock.log", "a") as f:
        f.write(msg + "\\n")

log("Mock server started")

while True:
    line = sys.stdin.readline()
    if not line:
        break
    log(f"Received: {line.strip()}")
    try:
        req = json.loads(line)
        if req.get("method") == "initialize":
            resp = {
                "jsonrpc": "2.0",
                "id": req.get("id"),
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "mock-server", "version": "1.0.0"}
                }
            }
            sys.stdout.write(json.dumps(resp) + "\\n")
            sys.stdout.flush()
            log("Sent initialize response")
        elif req.get("method") == "notifications/initialized":
             log("Received initialized notification")
        elif req.get("method") == "tools/list":
            resp = {
                "jsonrpc": "2.0",
                "id": req.get("id"),
                "result": {
                    "tools": []
                }
            }
            sys.stdout.write(json.dumps(resp) + "\\n")
            sys.stdout.flush()
            log("Sent tools/list response")
            # We can exit after tools/list for a simple handshake test
            sys.exit(0)
    except Exception as e:
        log(f"Error: {e}")
"""

def main() -> int:
    print("=== MCP HANDSHAKE E2E TEST ===")

    original_cwd = Path.cwd()
    proc = None
    with tempfile.TemporaryDirectory() as tmp_dir:
        tmp_path = Path(tmp_dir)
        os.chdir(tmp_path)
        try:

            # 1. Create Mock Server Script
            mock_script = tmp_path / "mock_mcp.py"
            mock_script.write_text(MOCK_SERVER_CODE)

            # 2. Create .ownstack/mcp_servers.json
            ownstack_dir = tmp_path / ".ownstack"
            ownstack_dir.mkdir()
            mcp_config = {
                "servers": [
                    {
                        "name": "mock",
                        "command": sys.executable,
                        "args": [str(mock_script)],
                        "enabled": True,
                    }
                ]
            }
            (ownstack_dir / "mcp_servers.json").write_text(json.dumps(mcp_config))

            # 3. Locate Agent
            is_windows = os.name == "nt"
            repo_root = Path(__file__).resolve().parents[2]
            agent_path = repo_root / "target" / "debug" / (
                "ownstack-agent.exe" if is_windows else "ownstack-agent"
            )

            if not agent_path.exists():
                print(f"Error: {agent_path} not found. Build the workspace first.")
                return 1

            # 4. Run Agent
            print(f"Spawning agent from {agent_path} in {tmp_path}")
            proc = subprocess.Popen(
                [str(agent_path), "--workspace", "."],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )

            # Wait for some time to allow handshake
            print("Waiting for handshake...")
            try:
                stdout, stderr = proc.communicate(timeout=10)
                print("Agent exited.")
            except subprocess.TimeoutExpired:
                print("Agent timeout (as expected, it stays alive in RPC mode)")
                proc.kill()
                stdout, stderr = proc.communicate()

            # 5. Check Mock Log
            log_file = tmp_path / "mcp_mock.log"
            if not log_file.exists():
                print("FAILURE: mcp_mock.log not found. Mock server never ran.")
                print("Agent stderr:", stderr)
                return 1

            logs = log_file.read_text()
            print("Mock Server Logs:")
            print(logs)

            if (
                "Sent initialize response" in logs
                and "Sent tools/list response" in logs
            ):
                print("SUCCESS: MCP Handshake completed successfully.")
                return 0

            print("FAILURE: Handshake sequence incomplete.")
            return 1
        finally:
            os.chdir(original_cwd)
            if proc is not None and proc.poll() is None:
                proc.terminate()
                try:
                    proc.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait(timeout=2)

if __name__ == "__main__":
    sys.exit(main())
