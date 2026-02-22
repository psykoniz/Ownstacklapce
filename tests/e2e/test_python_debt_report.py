#!/usr/bin/env python3
import subprocess
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT_PATH = REPO_ROOT / "scripts" / "report_python_debt.py"


def main() -> int:
    tmp_dir = REPO_ROOT / "target" / "tmp_test_python_debt_report"
    tmp_dir.mkdir(parents=True, exist_ok=True)

    metrics_path = tmp_dir / "metrics.jsonl"
    output_path = tmp_dir / "report.md"

    metrics_path.write_text(
        "\n".join(
            [
                '{"method":"tools.exec","success":true,"latency_ms":120}',
                '{"method":"tools.exec","success":false,"latency_ms":180,"error":"timeout"}',
                '{"method":"agent.plan","success":true,"latency_ms":40}',
            ]
        )
        + "\n",
        encoding="utf-8",
    )

    proc = subprocess.run(
        [
            sys.executable,
            str(SCRIPT_PATH),
            "--metrics",
            str(metrics_path),
            "--output",
            str(output_path),
        ],
        cwd=str(REPO_ROOT),
        capture_output=True,
        text=True,
        timeout=30,
    )
    if proc.returncode != 0:
        print(proc.stdout)
        print(proc.stderr)
        return 1

    if not output_path.exists():
        print("report output missing")
        return 1

    report = output_path.read_text(encoding="utf-8")
    if "`tools.exec`" not in report:
        print("tools.exec missing from report")
        return 1
    if "`agent.plan`" not in report:
        print("agent.plan missing from report")
        return 1
    if "Priority" not in report:
        print("priority column missing")
        return 1

    print("[PASS] python debt report generation")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
