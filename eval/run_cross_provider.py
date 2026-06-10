"""Cross-provider eval runner for Trust.

Runs the same task suite (tasks.toml) through a non-Anthropic model and saves
.rs files in eval/runs/<run-id>/ so the existing score.py + summarize.py
pipeline can score and aggregate them.

Usage:
    python3 eval/run_cross_provider.py \\
        --provider openai \\
        --model gpt-4o \\
        --run 005 \\
        --tasks 11-pipeline,12-result-chain,13-numeric,14-imports,15-many-points \\
        --trials 3

    python3 eval/run_cross_provider.py \\
        --provider gemini \\
        --model gemini-2.5-flash \\
        --run 006 \\
        --tasks 11-pipeline,12-result-chain,13-numeric,14-imports,15-many-points \\
        --trials 3

Task ids are the full string ids from tasks.toml (e.g. `11-pipeline`),
not bare numbers. An unknown id is a hard error — the runner refuses to
start rather than silently skipping it.

After running:
    python3 eval/score.py 005
    python3 eval/summarize.py 005
    cat eval/runs/005/summary.md

Requirements:
    - openai provider:  pip install openai;     OPENAI_API_KEY set
    - gemini provider:  pip install google-genai; GOOGLE_API_KEY set
"""

from __future__ import annotations

import argparse
import os
import sys
import tomllib
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parent

# ---------------------------------------------------------------------------
# Provider adapters
# ---------------------------------------------------------------------------

def call_openai(model: str, prompt: str) -> str:
    import openai  # type: ignore
    client = openai.OpenAI(api_key=os.environ["OPENAI_API_KEY"])
    response = client.chat.completions.create(
        model=model,
        messages=[{"role": "user", "content": prompt}],
        temperature=0.0,
    )
    return response.choices[0].message.content or ""


def _parse_gemini_retry_delay(err: Exception) -> float | None:
    """Pull `retryDelay` (e.g. "40s") out of a Gemini 429 error payload.

    Handles single-quoted dict repr ('retryDelay': '40s'), JSON ("retryDelay": "40s"),
    and the natural-language "Please retry in 40.620046432s." in the message.
    """
    import re
    blob = f"{getattr(err, 'message', '')} {err}"
    # Structured: 'retryDelay': '40s' or "retryDelay":"40s"
    m = re.search(r'retry[_-]?[Dd]elay[\'"\s:]+[\'"]?(\d+(?:\.\d+)?)s', blob)
    if m:
        return float(m.group(1))
    # Natural language: "Please retry in 40.620046432s."
    m = re.search(r'retry in\s+(\d+(?:\.\d+)?)s', blob, re.IGNORECASE)
    if m:
        return float(m.group(1))
    return None


class DailyQuotaExceeded(RuntimeError):
    """Raised when Gemini reports a per-day quota — retrying won't help today."""


def call_gemini(model: str, prompt: str) -> str:
    from google import genai  # type: ignore
    from google.genai import errors, types  # type: ignore
    client = genai.Client(api_key=os.environ["GOOGLE_API_KEY"])

    max_attempts = 6
    for attempt in range(1, max_attempts + 1):
        try:
            response = client.models.generate_content(
                model=model,
                contents=prompt,
                config=types.GenerateContentConfig(temperature=0.0),
            )
            return response.text or ""
        except errors.ClientError as exc:
            if getattr(exc, "code", None) != 429 or attempt == max_attempts:
                raise
            # Per-day quotas don't reset by sleeping — bail fast so the operator can react.
            if "PerDay" in str(exc):
                raise DailyQuotaExceeded(
                    "Gemini per-day quota exhausted; retrying will not help today. "
                    "Wait for the daily reset, upgrade your tier, or switch model."
                ) from exc
            delay = _parse_gemini_retry_delay(exc)
            if delay is None:
                delay = min(2 ** attempt, 60)
            delay += 1  # small cushion so we don't hit the same window again
            print(f" rate-limited, sleeping {delay:.0f}s (attempt {attempt}/{max_attempts - 1}) ...",
                  end="", flush=True, file=sys.stderr)
            time.sleep(delay)

    raise RuntimeError("call_gemini: exhausted retries without returning")


PROVIDERS: dict[str, dict] = {
    "openai": {
        "call": call_openai,
        "default_model": "gpt-4o",
        "env_key": "OPENAI_API_KEY",
    },
    "gemini": {
        "call": call_gemini,
        "default_model": "gemini-2.5-flash",
        "env_key": "GOOGLE_API_KEY",
    },
}


# ---------------------------------------------------------------------------
# Prompt extraction
# ---------------------------------------------------------------------------

def extract_source(raw: str) -> str:
    """Strip markdown fences if the model wrapped its output."""
    raw = raw.strip()
    if raw.startswith("```"):
        lines = raw.splitlines()
        # drop opening fence
        lines = lines[1:]
        # drop closing fence (last non-empty line that starts with ```)
        while lines and lines[-1].strip().startswith("```"):
            lines.pop()
        return "\n".join(lines).strip()
    return raw


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main() -> None:
    parser = argparse.ArgumentParser(description="Cross-provider Trust eval runner")
    parser.add_argument("--provider", required=True, choices=list(PROVIDERS),
                        help="LLM provider to use")
    parser.add_argument("--model", help="Model name (defaults to provider default)")
    parser.add_argument("--run", required=True, help="Run ID, e.g. 005")
    parser.add_argument(
        "--tasks",
        default="11-pipeline,12-result-chain,13-numeric,14-imports,15-many-points",
        help="Comma-separated task IDs (full ids from tasks.toml, e.g. 11-pipeline)")
    parser.add_argument("--trials", type=int, default=3,
                        help="Number of trials per task × condition")
    parser.add_argument("--conditions", default="vanilla,trust",
                        help="Comma-separated conditions to run")
    parser.add_argument("--context-file", type=Path, default=None,
                        help="Prepend this file's contents to every prompt "
                             "(e.g. skills/writing-trust/SKILL.md for the "
                             "skilled arm of the RT-97 suite). Use a separate "
                             "--run id per arm.")
    args = parser.parse_args()

    context = args.context_file.read_text().strip() if args.context_file else None

    provider_cfg = PROVIDERS[args.provider]
    model = args.model or provider_cfg["default_model"]
    env_key = provider_cfg["env_key"]
    call = provider_cfg["call"]

    if not os.environ.get(env_key):
        print(f"error: {env_key} is not set", file=sys.stderr)
        sys.exit(1)

    task_ids = [t.strip() for t in args.tasks.split(",")]
    conditions = [c.strip() for c in args.conditions.split(",")]

    tasks_raw = tomllib.loads((ROOT / "tasks.toml").read_text())["task"]
    tasks = {t["id"]: t for t in tasks_raw}

    unknown = [tid for tid in task_ids if tid not in tasks]
    if unknown:
        print(
            f"error: unknown task id(s): {', '.join(unknown)}\n"
            f"  valid ids: {', '.join(t['id'] for t in tasks_raw)}",
            file=sys.stderr,
        )
        sys.exit(2)

    run_dir = ROOT / "runs" / args.run
    run_dir.mkdir(parents=True, exist_ok=True)

    notes_path = run_dir / "NOTES.md"
    if not notes_path.exists():
        notes_path.write_text(
            f"# Run {args.run}\n\n"
            f"Provider: {args.provider}  \nModel: {model}  \n"
            f"Trials: {args.trials}  \n"
            f"Tasks: {args.tasks}  \n"
            f"Context file: {args.context_file or 'none (bare arm)'}  \n\n"
            "Generated by `eval/run_cross_provider.py`.\n"
        )

    total = len(task_ids) * len(conditions) * args.trials
    done = 0
    succeeded = 0  # new files written this run
    skipped = 0    # files that already existed
    failed = 0     # calls that raised

    for tid in task_ids:
        task = tasks[tid]

        for condition in conditions:
            prompt_key = f"{condition}_prompt"
            prompt = task.get(prompt_key)
            if not prompt:
                print(f"  skip {tid}/{condition}: no {prompt_key}", file=sys.stderr)
                continue

            for trial in range(1, args.trials + 1):
                out_path = run_dir / f"{tid}-{condition}-{trial}.rs"
                if out_path.exists():
                    print(f"  skip {out_path.name} (exists)")
                    skipped += 1
                    done += 1
                    continue

                print(f"  [{done+1}/{total}] {tid}-{condition}-{trial} ... ", end="", flush=True)
                prompt_text = prompt.strip()
                if context:
                    prompt_text = context + "\n\n---\n\n" + prompt_text
                try:
                    raw = call(model, prompt_text)
                    source = extract_source(raw)
                    out_path.write_text(source)
                    print("ok")
                    succeeded += 1
                except Exception as exc:
                    print(f"ERROR: {exc}", file=sys.stderr)
                    failed += 1
                done += 1

                # Gentle rate limiting between calls.
                if done < total:
                    time.sleep(0.5)

    # Summary line first — always printed, so an all-errored run can never
    # masquerade as success (RT-52).
    print(
        f"\nRun {args.run}: {succeeded} succeeded, {failed} failed"
        f"{f', {skipped} already existed' if skipped else ''}"
        f" (of {total} planned)."
    )
    if failed:
        print(
            f"error: {failed} call(s) failed — see ERROR lines above. "
            "Not emitting the scoring instructions for a partial run.",
            file=sys.stderr,
        )
        sys.exit(1)

    print("To score:")
    print(f"  python3 eval/score.py {args.run}")
    print(f"  python3 eval/summarize.py {args.run}")
    print(f"  cat eval/runs/{args.run}/summary.md")


if __name__ == "__main__":
    main()
