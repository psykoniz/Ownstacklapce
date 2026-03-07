import subprocess
import sys
import time
import os
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
E2E_DIR = REPO_ROOT / "tests" / "e2e"


def run_script(script_name, timeout_seconds=120):
    print(f"\n[HEALTHCHECK] Running {script_name}...")
    try:
        start_time = time.time()
        script_path = E2E_DIR / script_name
        result = subprocess.run(
            [sys.executable, str(script_path)],
            capture_output=True,
            text=True,
            cwd=str(REPO_ROOT),
            timeout=timeout_seconds,
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
    except subprocess.TimeoutExpired:
        print(f"[FAIL] {script_name} (Timed out after {timeout_seconds}s)")
        return False
    except Exception as err:
        print(f"[ERROR] Failed to run {script_name}: {err}")
        return False


def main():
    print("=== OWNSTACK IDE HEALTHCHECK ===")
    run_complex_missions = os.getenv("OWNSTACK_RUN_COMPLEX_MISSIONS", "").strip().lower() in {
        "1",
        "true",
        "yes",
        "on",
    }

    scripts = [
        "verify_agent_spawn.py",
        "verify_sandbox_exec.py",
        "verify_escape_mitigation.py",
        "test_wasi_plugin.py",
        "test_policy_approval.py",
        "test_agent_rpc.py",
        "test_mcp_handshake.py",
        "test_packaging_install_run.py",
        "verify_llm_e2e.py",
    ]
    if run_complex_missions:
        scripts.extend(
            [
                "test_mini_project_mission.py",
                "test_scraper_bot_mission.py",
            ]
        )
    script_timeouts = {
        # WASI plugin test can occasionally take longer on cold builds.
        "test_wasi_plugin.py": 240,
        # Complex mission tests have long end-to-end tool+LLM cycles.
        "test_mini_project_mission.py": 360,
        "test_scraper_bot_mission.py": 360,
    }

    llm_api_scripts = {
        "verify_llm_e2e.py",
        "test_agent_rpc.py",
        "test_mini_project_mission.py",
        "test_scraper_bot_mission.py",
    }
    has_keys = any(os.getenv(k) for k in ["ANTHROPIC_API_KEY", "OPENROUTER_API_KEY", "OPENAI_API_KEY"])

    all_passed = True
    for script in scripts:
        if script in llm_api_scripts and not has_keys:
            print(f"\n[SKIP] {script} (No API keys found in environment)")
            continue

        timeout_seconds = script_timeouts.get(script, 120)
        if not run_script(script, timeout_seconds=timeout_seconds):
            all_passed = False

    print("\n=== SUMMARY ===")
    if all_passed:
        print("ALL CHECKS PASSED. The system is healthy.")
        sys.exit(0)

    print("SOME CHECKS FAILED.")
    sys.exit(1)


if __name__ == "__main__":
    main()
