"""Agent-workflow eval runner for Trust (RT-97).

Runs the tasks in eval/agent-workflow/tasks.toml as short agent sessions:
copy a fixture crate into a work directory, hand the model the prompt plus
the file contents, write the files it proposes, run `cargo trustc build`,
feed failures back, and stop when the build is green or the round budget is
spent. Outputs land in eval/runs/<run-id>/<task>-<arm>-<trial>/ so
score_agent.py + summarize_agent.py can score and aggregate them.

Arms:
    bare     — the task prompt as written in tasks.toml
    skilled  — the same prompt with skills/writing-trust/SKILL.md prepended

Usage:
    python3 eval/agent-workflow/run_agent_eval.py \\
        --provider openai --model gpt-4o --run 007 --arm bare --trials 3
    python3 eval/agent-workflow/run_agent_eval.py \\
        --provider openai --model gpt-4o --run 007 --arm skilled --trials 3

After running both arms:
    python3 eval/agent-workflow/score_agent.py 007
    python3 eval/agent-workflow/summarize_agent.py 007
    cat eval/runs/007/summary.md

Use a fresh run id per provider+model — do not mix agent-workflow output
dirs with single-file .rs outputs in one run dir (the two suites have
different scorers).

Requirements:
    - the toolchain binaries, built once:
        cargo build -p trust-lang -p cargo-trustc -p trust-rustc -p trust-rustdoc
    - openai provider:  pip install openai;       OPENAI_API_KEY set
    - gemini provider:  pip install google-genai; GOOGLE_API_KEY set
    (Provider adapters are reused from ../run_cross_provider.py. For an
    Anthropic arm, spawn Haiku subagents from a Claude session into the same
    workdir layout, as in runs 001–004.)
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent  # eval/agent-workflow
EVAL = ROOT.parent  # eval/
REPO = EVAL.parent

sys.path.insert(0, str(EVAL))
from run_cross_provider import PROVIDERS  # noqa: E402  (shared provider adapters)

SKILL_PATH = REPO / "skills" / "writing-trust" / "SKILL.md"

# Model output protocol: every changed file as a `FILE:` header + fenced block.
FILE_BLOCK = re.compile(r"^FILE:\s*(\S+)\s*\n```[a-zA-Z]*\n(.*?)\n```", re.M | re.S)

RESPONSE_FORMAT = """\
Respond with every file you want to create or change, each in this exact
format (full file contents, not a diff):

FILE: <path relative to the crate root, e.g. src/main.rs or Cargo.toml>
```
<complete new contents of that file>
```

Only Cargo.toml and files under src/ may be changed. No other commentary is
needed.
"""


def trust_env() -> dict:
    """Environment for `cargo trustc` — shims on PATH, stale wrappers unset."""
    env = dict(os.environ)
    env.pop("RUSTC_WRAPPER", None)
    env.pop("RUSTDOC", None)
    env["PATH"] = f"{REPO / 'target' / 'debug'}{os.pathsep}{env.get('PATH', '')}"
    return env


def clear_lowering_cache() -> None:
    """Old wrappers share cache state (RT-86) — clear between builds."""
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
            "  Build them once:\n"
            "    cargo build -p trust-lang -p cargo-trustc -p trust-rustc -p trust-rustdoc",
            file=sys.stderr,
        )
        sys.exit(2)


def cargo_trust_build(workdir: Path) -> subprocess.CompletedProcess:
    clear_lowering_cache()
    return subprocess.run(
        ["cargo", "trust", "build"],
        cwd=workdir,
        env=trust_env(),
        capture_output=True,
        text=True,
    )


def copy_fixture(fixture: Path, workdir: Path) -> None:
    """Copy the fixture crate, excluding the agent-hidden reference solution."""
    shutil.copytree(
        fixture,
        workdir,
        ignore=shutil.ignore_patterns("reference-solution", "target", "Cargo.lock"),
    )


def workdir_files(workdir: Path) -> list[Path]:
    files = [workdir / "Cargo.toml"]
    files.extend(sorted((workdir / "src").rglob("*.rs")))
    return [f for f in files if f.is_file()]


def render_files(workdir: Path) -> str:
    chunks = []
    for f in workdir_files(workdir):
        rel = f.relative_to(workdir)
        chunks.append(f"FILE: {rel}\n```\n{f.read_text().rstrip()}\n```")
    return "\n\n".join(chunks)


def apply_files(workdir: Path, raw: str) -> list[str]:
    """Write FILE: blocks from the model's reply. Returns the paths written."""
    written: list[str] = []
    for rel, contents in FILE_BLOCK.findall(raw):
        rel = rel.strip().lstrip("./")
        # Only the crate manifest and sources are writable; no escapes.
        if rel != "Cargo.toml" and not rel.startswith("src/"):
            continue
        target = (workdir / rel).resolve()
        if not str(target).startswith(str(workdir.resolve()) + os.sep):
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(contents + "\n")
        written.append(rel)
    return written


def run_trial(
    task: dict,
    arm: str,
    skill_text: str | None,
    call,
    model: str,
    workdir: Path,
    rounds: int,
) -> dict:
    """Drive one agent session. Returns a transcript record."""
    copy_fixture(ROOT / "fixtures" / task["id"], workdir.parent / "tmp-fixture")
    (workdir.parent / "tmp-fixture").rename(workdir)

    parts: list[str] = []
    if arm == "skilled" and skill_text:
        parts.append(
            "You have the following skill document for working in this "
            "codebase:\n\n" + skill_text.strip() + "\n\n---\n"
        )
    parts.append(task["prompt"].strip())
    if task["kind"] == "remediation":
        pre = cargo_trust_build(workdir)
        parts.append("\nBuild output:\n```\n" + pre.stderr.strip() + "\n```")
    parts.append("\nCurrent files:\n\n" + render_files(workdir))
    parts.append("\n" + RESPONSE_FORMAT)

    transcript: list[dict] = [{"role": "user", "content": "\n".join(parts)}]
    green = False
    for round_no in range(1, rounds + 1):
        prompt = "\n\n".join(
            m["content"] for m in transcript if m["role"] == "user"
        )
        # Adapters are single-turn; replay assistant turns inline so the
        # model sees its own prior attempts.
        for m in transcript:
            if m["role"] == "assistant":
                prompt += "\n\n[Your previous reply]\n" + m["content"]
        raw = call(model, prompt)
        transcript.append({"role": "assistant", "content": raw})
        written = apply_files(workdir, raw)
        proc = cargo_trust_build(workdir)
        if proc.returncode == 0:
            green = True
            break
        feedback = (
            f"(round {round_no}) You changed: {', '.join(written) or 'nothing parseable'}. "
            "`cargo trustc build` still fails:\n```\n"
            + proc.stderr.strip()[-4000:]
            + "\n```\nReply with corrected files in the same FILE: format."
        )
        transcript.append({"role": "user", "content": feedback})

    return {"task": task["id"], "arm": arm, "rounds_used": round_no,
            "build_green": green, "transcript": transcript}


def main() -> None:
    parser = argparse.ArgumentParser(description="Agent-workflow Trust eval runner")
    parser.add_argument("--provider", required=True, choices=list(PROVIDERS))
    parser.add_argument("--model", help="Model name (defaults to provider default)")
    parser.add_argument("--run", required=True, help="Run ID, e.g. 007")
    parser.add_argument("--arm", required=True, choices=["bare", "skilled"])
    parser.add_argument("--tasks", default="30-setup-logstat,31-remediate-r0018,"
                        "32-remediate-r0019,33-remediate-r0020,34-remediate-r0021",
                        help="Comma-separated task IDs from agent-workflow/tasks.toml")
    parser.add_argument("--trials", type=int, default=3)
    parser.add_argument("--rounds", type=int, default=3,
                        help="Max build-fix rounds per trial")
    args = parser.parse_args()

    ensure_toolchain()

    provider_cfg = PROVIDERS[args.provider]
    model = args.model or provider_cfg["default_model"]
    if not os.environ.get(provider_cfg["env_key"]):
        print(f"error: {provider_cfg['env_key']} is not set", file=sys.stderr)
        sys.exit(1)
    call = provider_cfg["call"]

    tasks_raw = tomllib.loads((ROOT / "tasks.toml").read_text())["task"]
    tasks = {t["id"]: t for t in tasks_raw}
    task_ids = [t.strip() for t in args.tasks.split(",")]
    unknown = [tid for tid in task_ids if tid not in tasks]
    if unknown:
        print(
            f"error: unknown task id(s): {', '.join(unknown)}\n"
            f"  valid ids: {', '.join(tasks)}",
            file=sys.stderr,
        )
        sys.exit(2)

    skill_text = SKILL_PATH.read_text() if args.arm == "skilled" else None

    run_dir = EVAL / "runs" / args.run
    run_dir.mkdir(parents=True, exist_ok=True)
    notes_path = run_dir / "NOTES.md"
    if not notes_path.exists():
        notes_path.write_text(
            f"# Run {args.run} (agent-workflow suite)\n\n"
            f"Provider: {args.provider}  \nModel: {model}  \n"
            f"Trials: {args.trials}  \nRounds: {args.rounds}  \n\n"
            "Generated by `eval/agent-workflow/run_agent_eval.py`.\n"
        )

    total = len(task_ids) * args.trials
    done = succeeded = skipped = failed = 0
    for tid in task_ids:
        task = tasks[tid]
        for trial in range(1, args.trials + 1):
            workdir = run_dir / f"{tid}-{args.arm}-{trial}"
            if workdir.exists():
                print(f"  skip {workdir.name} (exists)")
                skipped += 1
                done += 1
                continue
            print(f"  [{done+1}/{total}] {workdir.name} ... ", end="", flush=True)
            try:
                record = run_trial(task, args.arm, skill_text, call, model,
                                   workdir, args.rounds)
                (workdir / "transcript.json").write_text(json.dumps(record, indent=2))
                print("green" if record["build_green"] else "red")
                succeeded += 1
            except Exception as exc:
                print(f"ERROR: {exc}", file=sys.stderr)
                failed += 1
            done += 1
            if done < total:
                time.sleep(0.5)

    # Summary line first — an all-errored run can never masquerade as success.
    print(
        f"\nRun {args.run} [{args.arm}]: {succeeded} completed, {failed} failed"
        f"{f', {skipped} already existed' if skipped else ''}"
        f" (of {total} planned)."
    )
    if failed:
        print(f"error: {failed} trial(s) failed — see ERROR lines above.",
              file=sys.stderr)
        sys.exit(1)

    print("To score (after running both arms):")
    print(f"  python3 eval/agent-workflow/score_agent.py {args.run}")
    print(f"  python3 eval/agent-workflow/summarize_agent.py {args.run}")
    print(f"  cat eval/runs/{args.run}/summary.md")


if __name__ == "__main__":
    main()
