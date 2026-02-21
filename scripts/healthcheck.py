
import subprocess
import sys
import time

def run_script(script_name):
    print(f"\n[HEALTHCHECK] Running {script_name}...")
    try:
        start_time = time.time()
        result = subprocess.run([sys.executable, f"scripts/{script_name}"], capture_output=True, text=True)
        duration = time.time() - start_time
        
        if result.returncode == 0:
            print(f"[PASS] {script_name} ({duration:.2f}s)")
            return True
        else:
            print(f"[FAIL] {script_name} (Exit Code: {result.returncode})")
            print("--- STDOUT ---")
            print(result.stdout)
            print("--- STDERR ---")
            print(result.stderr)
            return False
    except Exception as e:
        print(f"[ERROR] Failed to run {script_name}: {e}")
        return False

def main():
    print("=== OWNSTACK IDE HEALTHCHECK ===")
    
    scripts = [
        "verify_agent_spawn.py",
        "verify_sandbox_exec.py"
    ]
    
    all_passed = True
    for script in scripts:
        if not run_script(script):
            all_passed = False
            # break # Optional: stop on first failure? let's run all.
            
    print("\n=== SUMMARY ===")
    if all_passed:
        print("✅ ALL CHECKS PASSED. The system is healthy.")
        sys.exit(0)
    else:
        print("❌ SOME CHECKS FAILED.")
        sys.exit(1)

if __name__ == "__main__":
    main()
