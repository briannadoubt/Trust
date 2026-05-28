"""Aggregate .score.json files into eval/runs/<run-id>/summary.md."""

from __future__ import annotations

import json
import sys
import tomllib
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent

if len(sys.argv) != 2:
    print("usage: summarize.py <run-id>", file=sys.stderr)
    sys.exit(2)

run_dir = ROOT / "runs" / sys.argv[1]
tasks_raw = tomllib.loads((ROOT / "tasks.toml").read_text())["task"]
task_order = [t["id"] for t in tasks_raw]
task_desc = {t["id"]: t["description"] for t in tasks_raw}

scores: list[dict] = []
for f in sorted(run_dir.glob("*.score.json")):
    scores.append(json.loads(f.read_text()))

grouped: dict = defaultdict(lambda: defaultdict(list))
for s in scores:
    grouped[s["task"]][s["condition"]].append(s)


def fmt_frac(num: int, den: int) -> str:
    if den == 0:
        return "—"
    pct = 100 * num // den
    return f"{num}/{den} ({pct}%)"


lines: list[str] = []
lines.append(f"# Trust eval — run {sys.argv[1]}")
lines.append("")
lines.append(
    "*Bug in source* — agent typed the known-bad pattern. "
    "*Caught* — `trust check` flagged it (only meaningful in the trust condition). "
    "*Shipped* — bug present **and** dialect did not catch it. "
    "Shipped is the headline column."
)
lines.append("")
lines.append("| Task | Condition | Trials | Bug in source | Caught | Shipped |")
lines.append("|------|-----------|--------|---------------|--------|---------|")

for tid in task_order:
    for cond in ("vanilla", "trust"):
        trials = grouped[tid][cond]
        n = len(trials)
        if n == 0:
            continue
        bug = sum(1 for t in trials if t["bug_in_source"])
        if cond == "trust":
            caught = sum(1 for t in trials if t["dialect_caught"])
            caught_str = fmt_frac(caught, n)
        else:
            caught_str = "—"
        shipped = sum(1 for t in trials if t["shipped"])
        lines.append(
            f"| `{tid}` | {cond} | {n} | {fmt_frac(bug, n)} | {caught_str} | **{fmt_frac(shipped, n)}** |"
        )

lines.append("")

vanilla_all = [s for s in scores if s["condition"] == "vanilla"]
rust_all = [s for s in scores if s["condition"] == "trust"]
v_ship = sum(1 for s in vanilla_all if s["shipped"])
r_ship = sum(1 for s in rust_all if s["shipped"])
v_bug = sum(1 for s in vanilla_all if s["bug_in_source"])
r_bug = sum(1 for s in rust_all if s["bug_in_source"])
r_caught = sum(1 for s in rust_all if s["dialect_caught"])

lines.append("## Totals")
lines.append("")
lines.append(f"- **Vanilla**: {fmt_frac(v_bug, len(vanilla_all))} had the bug in source; {fmt_frac(v_ship, len(vanilla_all))} shipped.")
lines.append(f"- **Trust**: {fmt_frac(r_bug, len(rust_all))} had the bug in source; {fmt_frac(r_caught, len(rust_all))} caught; **{fmt_frac(r_ship, len(rust_all))} shipped**.")

if vanilla_all and rust_all:
    v_rate = v_ship / len(vanilla_all)
    r_rate = r_ship / len(rust_all)
    if v_rate > 0:
        reduction = (v_rate - r_rate) / v_rate * 100
        lines.append("")
        lines.append(
            f"- **Bug-ship reduction**: vanilla ships at {v_rate*100:.0f}%, "
            f"trust at {r_rate*100:.0f}% — {reduction:.0f}% fewer bugs reach prod."
        )

out_path = run_dir / "summary.md"
out_path.write_text("\n".join(lines) + "\n")
print(out_path.read_text())
