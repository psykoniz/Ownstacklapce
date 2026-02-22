#!/usr/bin/env python3
"""
Aggregate Python bridge usage metrics and generate a prioritized debt table.

Input format:
  JSONL events produced by ownstack-bridge at .ownstack/python_bridge_metrics.jsonl
"""

from __future__ import annotations

import argparse
import json
import math
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


@dataclass
class EndpointStats:
    calls: int = 0
    errors: int = 0
    latency_ms_values: list[float] = field(default_factory=list)

    def record(self, latency_ms: float, success: bool) -> None:
        self.calls += 1
        if not success:
            self.errors += 1
        self.latency_ms_values.append(latency_ms)

    @property
    def avg_latency_ms(self) -> float:
        if not self.latency_ms_values:
            return 0.0
        return sum(self.latency_ms_values) / len(self.latency_ms_values)

    @property
    def p95_latency_ms(self) -> float:
        if not self.latency_ms_values:
            return 0.0
        sorted_values = sorted(self.latency_ms_values)
        index = math.ceil(0.95 * len(sorted_values)) - 1
        index = max(0, min(index, len(sorted_values) - 1))
        return sorted_values[index]

    @property
    def error_rate(self) -> float:
        if self.calls == 0:
            return 0.0
        return self.errors / self.calls

    @property
    def impact_score(self) -> float:
        # Weighted blend: traffic + reliability + tail latency.
        return (
            float(self.calls)
            + float(self.errors) * 6.0
            + self.error_rate * 100.0
            + self.p95_latency_ms / 25.0
        )


def parse_event(line: str, line_number: int) -> tuple[str, float, bool]:
    payload = json.loads(line)
    method = str(payload.get("method", "")).strip()
    if not method:
        raise ValueError(f"line {line_number}: missing method")

    latency_ms_raw = payload.get("latency_ms", 0.0)
    latency_ms = float(latency_ms_raw)
    success = bool(payload.get("success", False))
    return method, latency_ms, success


def infer_component(method: str) -> str:
    if ":" in method:
        return method.split(":", 1)[0]
    if "." in method:
        return method.split(".", 1)[0]
    return "bridge"


def infer_rust_target(component: str) -> str:
    mapping = {
        "agent": "ownstack-agent/src/orchestrator.rs",
        "tools": "ownstack-agent/src/toolkits/",
        "session": "ownstack-agent/src/toolkits/core.rs",
        "missions": "ownstack-agent/src/orchestrator.rs",
        "monitoring": "ownstack-engine/src/",
        "gateway": "lapce-proxy/src/dispatch.rs",
        "bridge": "ownstack-bridge/src/lib.rs",
    }
    return mapping.get(component, "ownstack-agent/src/toolkits/")


def priority_label(score: float) -> str:
    if score >= 120:
        return "P0"
    if score >= 60:
        return "P1"
    if score >= 20:
        return "P2"
    return "P3"


def build_markdown(
    metrics_path: Path,
    stats_by_method: dict[str, EndpointStats],
) -> str:
    now = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%SZ")
    lines: list[str] = []
    lines.append("# Top Dettes Python Restantes")
    lines.append("")
    lines.append(f"- Generated at: `{now}`")
    lines.append(f"- Metrics source: `{metrics_path}`")
    lines.append("")

    if not stats_by_method:
        lines.append("Aucune métrique trouvée. Lancez des scénarios E2E puis régénérez ce rapport.")
        lines.append("")
        lines.append("Commande:")
        lines.append("```bash")
        lines.append("python scripts/report_python_debt.py")
        lines.append("```")
        return "\n".join(lines) + "\n"

    ranked = sorted(
        stats_by_method.items(),
        key=lambda item: item[1].impact_score,
        reverse=True,
    )

    total_calls = sum(stats.calls for stats in stats_by_method.values())
    total_errors = sum(stats.errors for stats in stats_by_method.values())
    lines.append(f"- Total calls observed: `{total_calls}`")
    lines.append(f"- Total errors observed: `{total_errors}`")
    lines.append("")
    lines.append("| Rank | Endpoint | Component | Calls | Errors | Err % | Avg ms | P95 ms | Impact | Priority | Rust target |")
    lines.append("|---:|---|---|---:|---:|---:|---:|---:|---:|---|---|")

    for index, (method, stats) in enumerate(ranked, start=1):
        component = infer_component(method)
        rust_target = infer_rust_target(component)
        lines.append(
            "| {rank} | `{method}` | `{component}` | {calls} | {errors} | {err_pct:.1f} | "
            "{avg:.1f} | {p95:.1f} | {impact:.1f} | `{priority}` | `{target}` |".format(
                rank=index,
                method=method,
                component=component,
                calls=stats.calls,
                errors=stats.errors,
                err_pct=stats.error_rate * 100.0,
                avg=stats.avg_latency_ms,
                p95=stats.p95_latency_ms,
                impact=stats.impact_score,
                priority=priority_label(stats.impact_score),
                target=rust_target,
            )
        )

    lines.append("")
    lines.append("Score impact utilisé: `calls + errors*6 + error_rate*100 + p95_latency_ms/25`.")
    return "\n".join(lines) + "\n"


def generate_report(metrics_path: Path, output_path: Path) -> int:
    stats_by_method: dict[str, EndpointStats] = {}

    if metrics_path.exists():
        for line_number, raw_line in enumerate(
            metrics_path.read_text(encoding="utf-8").splitlines(), start=1
        ):
            line = raw_line.strip()
            if not line:
                continue
            try:
                method, latency_ms, success = parse_event(line, line_number)
            except Exception:
                # Ignore malformed lines to keep report generation robust.
                continue

            endpoint_stats = stats_by_method.setdefault(method, EndpointStats())
            endpoint_stats.record(latency_ms=latency_ms, success=success)

    report = build_markdown(metrics_path, stats_by_method)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(report, encoding="utf-8")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Generate a prioritized Python debt report from bridge metrics."
    )
    parser.add_argument(
        "--metrics",
        type=Path,
        default=Path(".ownstack") / "python_bridge_metrics.jsonl",
        help="Path to bridge metrics JSONL file.",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("docs") / "TOP_PYTHON_DEBTS.md",
        help="Markdown output path.",
    )
    args = parser.parse_args()
    return generate_report(args.metrics, args.output)


if __name__ == "__main__":
    raise SystemExit(main())
