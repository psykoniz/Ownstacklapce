import subprocess
import sys
import time
import os
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
E2E_DIR = REPO_ROOT / "tests" / "e2e"


def run_script(script_name):
    print(f"\n[HEALTHCHECK] Running {script_name}...")
    try:
        start_time = time.time()
        script_path = E2E_DIR / script_name
        result = subprocess.run(
            [sys.executable, str(script_path)],
            capture_output=True,
            text=True,
            cwd=str(REPO_ROOT),
        )
        duration = time.time() - start_time

        if result.returncode == 0:
            print(f"[PASS] {script_name} ({duration:.2f}s)")
            return True

        print(f"[FAIL] {script_name} (Exit Code: {result.returncode})")
        print("--- STDOUT ---")
        print(result.stdout)
        print("--- STDERR ---")
        print(result.stderr)
        return False
    except Exception as err:
        print(f"[ERROR] Failed to run {script_name}: {err}")
        return False


def main():
    print("=== OWNSTACK IDE HEALTHCHECK ===")

    scripts = [
        "verify_agent_spawn.py",
        "verify_sandbox_exec.py",
        "test_wasi_plugin.py",
        "test_policy_approval.py",
        "test_agent_rpc.py",
        "test_mcp_handshake.py",
        "test_packaging_install_run.py",
        "verify_llm_e2e.py",
    ]

    all_passed = True
    for script in scripts:
        if script == "verify_llm_e2e.py":
            has_keys = any(os.getenv(k) for k in ["ANTHROPIC_API_KEY", "OPENROUTER_API_KEY", "OPENAI_API_KEY"])
            if not has_keys:
                print(f"\n[SKIP] {script} (No API keys found in environment)")
                continue
        
        if not run_script(script):
            all_passed = False

    print("\n=== SUMMARY ===")
    if all_passed:
        print("ALL CHECKS PASSED. The system is healthy.")
        sys.exit(0)

    print("SOME CHECKS FAILED.")
    sys.exit(1)


if __name__ == "__main__":
    main()
