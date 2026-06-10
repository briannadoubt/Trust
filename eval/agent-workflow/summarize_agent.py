"""Aggregate agent-workflow .score.json files into eval/runs/<run-id>/summary.md."""

from __future__ import annotations

import json
import sys
import tomllib
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent
EVAL = ROOT.parent

if len(sys.argv) != 2:
    print("usage: summarize_agent.py <run-id>", file=sys.stderr)
    sys.exit(2)

run_dir = EVAL / "runs" / sys.argv[1]
tasks_raw = tomllib.loads((ROOT / "tasks.toml").read_text())["task"]
task_order = [t["id"] for t in tasks_raw]

scores: list[dict] = []
for f in sorted(run_dir.glob("*.score.json")):
    s = json.loads(f.read_text())
    if "arm" in s:  # ignore single-file scores if a run dir was ever mixed
        scores.append(s)

grouped: dict = defaultdict(lambda: defaultdict(list))
for s in scores:
    grouped[s["task"]][s["arm"]].append(s)


def fmt_frac(num: int, den: int) -> str:
    if den == 0:
        return "—"
    pct = 100 * num // den
    return f"{num}/{den} ({pct}%)"


lines: list[str] = []
lines.append(f"# Trust agent-workflow eval — run {sys.argv[1]}")
lines.append("")
lines.append(
    "*Green* — `cargo trust build` exits 0 on the final work tree. "
    "*Anti-pattern gone* — the task's bug regex no longer matches. "
    "*Passed* — green ∧ anti-pattern gone ∧ fix idiom present "
    "(setup additionally: metadata key, no per-file markers, plain cargo still fails). "
    "Passed is the headline column; the skilled arm injects "
    "skills/writing-trust/SKILL.md, the bare arm does not."
)
lines.append("")
lines.append("| Task | Arm | Trials | Green | Anti-pattern gone | Passed |")
lines.append("|------|-----|--------|-------|-------------------|--------|")

for tid in task_order:
    for arm in ("bare", "skilled"):
        trials = grouped[tid][arm]
        n = len(trials)
        if n == 0:
            continue
        green = sum(1 for t in trials if t["builds_green"])
        gone = sum(1 for t in trials if not t["bug_in_source"])
        passed = sum(1 for t in trials if t["passed"])
        lines.append(
            f"| `{tid}` | {arm} | {n} | {fmt_frac(green, n)} | "
            f"{fmt_frac(gone, n)} | **{fmt_frac(passed, n)}** |"
        )

lines.append("")
lines.append("## Totals")
lines.append("")
for arm in ("bare", "skilled"):
    arm_scores = [s for s in scores if s["arm"] == arm]
    if not arm_scores:
        continue
    n = len(arm_scores)
    green = sum(1 for s in arm_scores if s["builds_green"])
    passed = sum(1 for s in arm_scores if s["passed"])
    lines.append(
        f"- **{arm}**: {fmt_frac(green, n)} built green; "
        f"**{fmt_frac(passed, n)} passed**."
    )

bare_all = [s for s in scores if s["arm"] == "bare"]
skilled_all = [s for s in scores if s["arm"] == "skilled"]
if bare_all and skilled_all:
    b_rate = sum(1 for s in bare_all if s["passed"]) / len(bare_all)
    s_rate = sum(1 for s in skilled_all if s["passed"]) / len(skilled_all)
    lines.append("")
    lines.append(
        f"- **Skill lift**: bare passes at {b_rate*100:.0f}%, "
        f"skilled at {s_rate*100:.0f}% — {(s_rate-b_rate)*100:+.0f} points."
    )

out_path = run_dir / "summary.md"
out_path.write_text("\n".join(lines) + "\n")
print(out_path.read_text())
