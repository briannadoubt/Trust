"""Score an agent-workflow eval run.

Usage: python3 eval/agent-workflow/score_agent.py <run-id>

Reads <task>-<arm>-<trial>/ work directories from eval/runs/<run-id>/,
re-runs `cargo trustc build` in each (cache cleared, shims on PATH), applies
the bug/good regexes from agent-workflow/tasks.toml to the final sources,
and writes <task>-<arm>-<trial>.score.json next to each directory — same
file naming as ../score.py uses for single-file trials.

Pass criteria:
    remediation: builds green  AND  bug regex gone  AND  good regex present
    setup:       all of the above  AND  trust metadata key in Cargo.toml
                 AND no per-file `#![strict]` markers
                 AND plain `cargo build` still FAILS (named args were kept,
                 not stripped to appease stock rustc)

Like ../score.py, this refuses to run without the toolchain binaries rather
than silently scoring every trial as red (the RT-53 lesson).
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent  # eval/agent-workflow
EVAL = ROOT.parent
REPO = EVAL.parent

METADATA_KEY = r"\[(?:package|workspace)\.metadata\.trust\]"
STRICT_MARKER = r"#!\[strict\]"


def trust_env() -> dict:
    env = dict(os.environ)
    env.pop("RUSTC_WRAPPER", None)
    env.pop("RUSTDOC", None)
    env["PATH"] = f"{REPO / 'target' / 'debug'}{os.pathsep}{env.get('PATH', '')}"
    return env


def plain_env() -> dict:
    env = dict(os.environ)
    env.pop("RUSTC_WRAPPER", None)
    env.pop("RUSTDOC", None)
    return env


def clear_lowering_cache() -> None:
    for base in (os.environ.get("TMPDIR"), "/tmp"):
        if base:
            shutil.rmtree(Path(base) / "trust-cache", ignore_errors=True)


def ensure_toolchain() -> None:
    missing = [
        b for b in ("cargo-trustc", "trust-rustc", "trust-rustdoc", "trust")
        if not (REPO / "target" / "debug" / b).exists()
    ]
    if missing:
        print(
            f"error: missing toolchain binaries in target/debug: {', '.join(missing)}\n"
            "  Scoring without them would fake an all-red run (RT-53). Build them:\n"
            "    cargo build -p trust-lang -p cargo-trustc -p trust-rustc -p trust-rustdoc",
            file=sys.stderr,
        )
        sys.exit(2)


def score_workdir(workdir: Path, task: dict) -> dict:
    stem = workdir.name  # "31-remediate-r0018-bare-2"
    task_id, arm, trial = stem.rsplit("-", 2)

    sources = "\n".join(
        p.read_text() for p in sorted((workdir / "src").rglob("*.rs"))
    )
    manifest = (workdir / "Cargo.toml").read_text()

    clear_lowering_cache()
    trust_proc = subprocess.run(
        ["cargo", "trust", "build"],
        cwd=workdir, env=trust_env(), capture_output=True, text=True,
    )
    builds_green = trust_proc.returncode == 0

    bug_in_source = bool(re.search(task["bug"], sources))
    good_in_source = bool(re.search(task["good"], sources))
    passed = builds_green and not bug_in_source and good_in_source

    score = {
        "file": workdir.name,
        "task": task_id,
        "arm": arm,
        "trial": int(trial),
        "kind": task["kind"],
        "builds_green": builds_green,
        "bug_in_source": bug_in_source,
        "good_in_source": good_in_source,
        "build_stderr": trust_proc.stderr[-4000:],
    }

    if task["kind"] == "setup":
        metadata_key = bool(re.search(METADATA_KEY, manifest))
        per_file_marker = bool(re.search(STRICT_MARKER, sources))
        plain_proc = subprocess.run(
            ["cargo", "build"],
            cwd=workdir, env=plain_env(), capture_output=True, text=True,
        )
        plain_cargo_fails = plain_proc.returncode != 0
        score.update(
            metadata_key=metadata_key,
            per_file_marker=per_file_marker,
            plain_cargo_fails=plain_cargo_fails,
        )
        passed = (
            passed and metadata_key and not per_file_marker and plain_cargo_fails
        )

    score["passed"] = passed
    return score


def main() -> None:
    parser = argparse.ArgumentParser(description="Score an agent-workflow eval run.")
    parser.add_argument("run_id", help="Run ID, e.g. 007")
    args = parser.parse_args()

    run_dir = EVAL / "runs" / args.run_id
    if not run_dir.is_dir():
        print(f"no such run dir: {run_dir}", file=sys.stderr)
        sys.exit(2)

    ensure_toolchain()

    tasks_raw = tomllib.loads((ROOT / "tasks.toml").read_text())["task"]
    tasks = {t["id"]: t for t in tasks_raw}

    workdirs = sorted(
        d for d in run_dir.iterdir()
        if d.is_dir() and d.name.rsplit("-", 2)[0] in tasks
    )
    if not workdirs:
        print(f"no agent-workflow work directories in {run_dir}", file=sys.stderr)
        sys.exit(2)

    for workdir in workdirs:
        task = tasks[workdir.name.rsplit("-", 2)[0]]
        score = score_workdir(workdir, task)
        out_path = run_dir / f"{workdir.name}.score.json"
        out_path.write_text(json.dumps(score, indent=2))
        print(
            f"{workdir.name:36s}  green={score['builds_green']!s:5}  "
            f"bug={score['bug_in_source']!s:5}  good={score['good_in_source']!s:5}  "
            f"passed={score['passed']}"
        )


if __name__ == "__main__":
    main()
