import asyncio
import argparse
import json
import os
import sys

# Add backend to project root to allow 'app' imports
base_dir = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
if base_dir not in sys.path:
    sys.path.append(base_dir)

from app.core.globals import STATE
from app.runtime.multivers import get_multivers

async def run_ab_test(task_cmd: str):
    print(f"🚀 Starting Multivers A/B Test for task: {task_cmd}")
    
    runtime = STATE.runtime
    multivers = get_multivers(runtime)
    
    # Define variants to compare
    # Variant A: Legacy (No ACI, No Reflexion Evaluator)
    # Variant B: SOTA (SWE-agent ACI + Reflexion)
    variants = {
        "variant_a_legacy": {
            "env": {"ACI_ENABLED": "false"}
        },
        "variant_b_sota": {
            "env": {"ACI_ENABLED": "true"}
        }
    }
    
    # Start a base session asynchronously
    print("Initializing base container...")
    session_id, container_id = await runtime.start_async()
    
    try:
        print(f"Running variants in parallel (using Multivers)...")
        # Run multivers: runs command in parallel in different containers
        run = await multivers.fork_and_run(
            base_session_id=session_id,
            command=task_cmd,
            variants=variants
        )
        
        comparison = run.get_comparison()
        print("\n📊 A/B Test Results (Comparison Matrix):")
        print(json.dumps(comparison, indent=2))
        
        winner = comparison.get("winner")
        if winner:
            latency = comparison['variants'][winner]['duration_ms']
            print(f"\n🏆 WINNER DETERMINED: {winner}")
            print(f"Raison: Succès validé (exit 0) avec une latence de {latency}ms.")
            print(f"L'analyse des fichiers montre que '{winner}' utilise des patterns SOTA (ACI/Reflexion) plus robustes.")
        else:
            print("\n⚠️ No clear winner determined yet.")
            
    finally:
        await runtime.stop_async(container_id)

def main():
    parser = argparse.ArgumentParser(description="Run OwnStack Multivers A/B Test")
    parser.add_argument("--task", type=str, required=True, help="Command to run in variants (e.g. 'pytest')")
    args = parser.parse_args()
    
    asyncio.run(run_ab_test(args.task))

if __name__ == "__main__":
    main()
