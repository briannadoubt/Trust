"""Score a single eval run.

Usage: python3 eval/score.py <run-id> [--build]

Reads .rs files from eval/runs/<run-id>/, applies the bug/good regex
from tasks.toml, runs `trust check` for the trust condition,
and writes a .score.json next to each .rs file.

The `trust` condition needs a *release* `trust` binary. Because the
`trust` crate dogfoods named-arg syntax (RT-31), that binary only
compiles under the lowering wrapper. Build it once with:

    cargo build -p trust-rustc
    RUSTC_WRAPPER="$(pwd)/target/debug/trust-rustc" cargo build --release -p trust-lang

If the binary is missing, this script refuses to run rather than
silently scoring every trust file as caught=✗ (the RT-53 bug, which
faked a 0/15 catch rate on run 005's first pass). Pass `--build` to run
the two commands above automatically, or set `TRUST_BIN` to point at an
existing binary.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parent


def trust_bin() -> Path:
    """Path to the release `trust` binary (overridable via TRUST_BIN)."""
    override = os.environ.get("TRUST_BIN")
    if override:
        return Path(override)
    return REPO / "target" / "release" / "trust"


def build_trust_binary() -> None:
    """Bootstrap the release `trust` binary under the lowering wrapper."""
    print("building trust-rustc wrapper (debug) ...", file=sys.stderr)
    subprocess.run(["cargo", "build", "-p", "trust-rustc"], cwd=REPO, check=True)
    wrapper = REPO / "target" / "debug" / "trust-rustc"
    print("building trust (release) under RUSTC_WRAPPER ...", file=sys.stderr)
    env = {**os.environ, "RUSTC_WRAPPER": str(wrapper)}
    subprocess.run(
        ["cargo", "build", "--release", "-p", "trust"],
        cwd=REPO,
        env=env,
        check=True,
    )


def ensure_trust_binary(*, build: bool) -> Path:
    """Return the trust binary path, building or failing loudly if absent."""
    binary = trust_bin()
    if binary.exists():
        return binary
    if build:
        build_trust_binary()
        if binary.exists():
            return binary
    print(
        f"error: release trust binary not found at {binary}\n"
        "  The `trust` condition can't be scored without it, and scoring\n"
        "  without it would fake a 0% catch rate (RT-53).\n\n"
        "  Build it once:\n"
        "    cargo build -p trust-rustc\n"
        '    RUSTC_WRAPPER="$(pwd)/target/debug/trust-rustc" '
        "cargo build --release -p trust-lang\n\n"
        "  ...or re-run this script with --build to do that automatically,\n"
        "  or set TRUST_BIN to point at an existing binary.",
        file=sys.stderr,
    )
    sys.exit(2)


def score_file(rs_path: Path, tasks: dict, trust: Path) -> dict:
    stem = rs_path.stem  # "11-pipeline-vanilla-3" or legacy "01-duration-vanilla"
    parts = stem.rsplit("-", 2)
    if len(parts) == 3 and parts[2].isdigit():
        task_id, condition, trial = parts[0], parts[1], int(parts[2])
    else:
        # Legacy single-trial filename: <task>-<condition>.rs
        last_dash = stem.rfind("-")
        task_id = stem[:last_dash]
        condition = stem[last_dash + 1 :]
        trial = 1
    task = tasks[task_id]

    source = rs_path.read_text()
    bug_in_source = bool(re.search(task["bug"], source))
    good_in_source = bool(re.search(task["good"], source))

    dialect_caught = None
    lint_stderr = None
    if condition == "trust":
        proc = subprocess.run(
            [str(trust), "check", str(rs_path)],
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
        "trial": trial,
        "bug_in_source": bug_in_source,
        "good_in_source": good_in_source,
        "dialect_caught": dialect_caught,
        "expected_rule": task["expected_rule"],
        "shipped": shipped,
        "lint_stderr": lint_stderr,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description="Score a single eval run.")
    parser.add_argument("run_id", help="Run ID, e.g. 005")
    parser.add_argument(
        "--build",
        action="store_true",
        help="Bootstrap the release trust binary if it is missing.",
    )
    args = parser.parse_args()

    run_dir = ROOT / "runs" / args.run_id
    if not run_dir.is_dir():
        print(f"no such run dir: {run_dir}", file=sys.stderr)
        sys.exit(2)

    tasks_raw = tomllib.loads((ROOT / "tasks.toml").read_text())["task"]
    tasks = {t["id"]: t for t in tasks_raw}

    rs_files = sorted(run_dir.glob("*.rs"))
    # Only require the binary when there's a trust-condition file to check.
    needs_trust = any("-trust-" in f.name or f.stem.endswith("-trust") for f in rs_files)
    trust = ensure_trust_binary(build=args.build) if needs_trust else trust_bin()

    for rs in rs_files:
        score = score_file(rs, tasks, trust)
        out_path = rs.with_suffix(".score.json")
        out_path.write_text(json.dumps(score, indent=2))
        caught = score["dialect_caught"]
        caught_str = "—" if caught is None else ("✓" if caught else "✗")
        print(
            f"{rs.name:40s}  bug={score['bug_in_source']!s:5}  "
            f"good={score['good_in_source']!s:5}  "
            f"caught={caught_str}  shipped={score['shipped']}"
        )


if __name__ == "__main__":
    main()
