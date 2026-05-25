"""Score a single eval run.

Usage: python3 eval/score.py <run-id>

Reads .rs files from eval/runs/<run-id>/, applies the bug/good regex
from tasks.toml, runs `rustricted check` for the rustricted condition,
and writes a .score.json next to each .rs file.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parent

if len(sys.argv) != 2:
    print("usage: score.py <run-id>", file=sys.stderr)
    sys.exit(2)

run_dir = ROOT / "runs" / sys.argv[1]
if not run_dir.is_dir():
    print(f"no such run dir: {run_dir}", file=sys.stderr)
    sys.exit(2)

tasks_raw = tomllib.loads((ROOT / "tasks.toml").read_text())["task"]
tasks = {t["id"]: t for t in tasks_raw}


def score_file(rs_path: Path) -> dict:
    stem = rs_path.stem  # e.g. "01-duration-vanilla"
    last_dash = stem.rfind("-")
    task_id = stem[:last_dash]
    condition = stem[last_dash + 1 :]
    task = tasks[task_id]

    source = rs_path.read_text()
    bug_in_source = bool(re.search(task["bug"], source))
    good_in_source = bool(re.search(task["good"], source))

    dialect_caught = None
    lint_stderr = None
    if condition == "rustricted":
        proc = subprocess.run(
            [
                "cargo",
                "run",
                "-q",
                "--release",
                "-p",
                "rustricted",
                "--",
                "check",
                str(rs_path),
            ],
            cwd=REPO,
            capture_output=True,
            text=True,
        )
        lint_stderr = proc.stderr
        dialect_caught = task["expected_rule"] in proc.stderr

    shipped = bug_in_source and not (dialect_caught or False)

    return {
        "file": rs_path.name,
        "task": task_id,
        "condition": condition,
        "bug_in_source": bug_in_source,
        "good_in_source": good_in_source,
        "dialect_caught": dialect_caught,
        "expected_rule": task["expected_rule"],
        "shipped": shipped,
        "lint_stderr": lint_stderr,
    }


for rs in sorted(run_dir.glob("*.rs")):
    score = score_file(rs)
    out_path = rs.with_suffix(".score.json")
    out_path.write_text(json.dumps(score, indent=2))
    caught = score["dialect_caught"]
    caught_str = "—" if caught is None else ("✓" if caught else "✗")
    print(
        f"{rs.name:40s}  bug={score['bug_in_source']!s:5}  "
        f"good={score['good_in_source']!s:5}  "
        f"caught={caught_str}  shipped={score['shipped']}"
    )
