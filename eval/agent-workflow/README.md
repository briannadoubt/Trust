# Trust agent-workflow eval (RT-97)

Extends the single-file completion eval (../README.md) along two axes:

1. **Does the `writing-trust` skill help?** Every task runs in two arms —
   *bare* (prompt only) and *skilled* (the full contents of
   `skills/writing-trust/SKILL.md` injected ahead of the prompt). The
   headline metric is the pass-rate delta between arms.
2. **Validation debt for R0018/R0019/R0020/R0021** (tickets RT-68/72/73/74):
   the four Tier 1/3 rules shipped without eval evidence. The remediation
   tasks here exercise each rule's fixture + `instead:` guidance end to end,
   and two new authoring tasks in `../tasks.toml` measure whether agents
   write those bug classes unprompted.

## Task categories

| Tasks | Category | What a trial is | Scored by |
|-------|----------|-----------------|-----------|
| `30-setup-logstat` | setup | Convert a plain-Rust fixture crate to Trust (metadata key, fix violations, build green) | `score_agent.py` |
| `31…34-remediate-r00XX` | remediation | Fix a crate that fails `cargo trustc build` with exactly R0018/19/20/21 | `score_agent.py` |
| `16-frame-parse`, `17-async-cache` | authoring | Greenfield single-file program whose natural solution tempts R0019 / R0020 | existing `../score.py` (shipped/caught) |

Setup and remediation tasks live in `tasks.toml` here (same `[[task]]` +
`bug`/`good` regex shape as `../tasks.toml`); a trial is a short agent
session driven by `run_agent_eval.py`: propose files → `cargo trustc build` →
read diagnostics → repeat, up to `--rounds` (default 3).

Authoring tasks live in `../tasks.toml` (ids `16-frame-parse`,
`17-async-cache`) so the existing `run_cross_provider.py` → `score.py` →
`summarize.py` pipeline runs them unchanged.

Pass criteria:

- **remediation** — `cargo trustc build` green ∧ the anti-pattern regex is
  gone ∧ the fix idiom regex is present (e.g. no `map_err(|_|`, a
  `checked_`/`saturating_` call, guard scoped or tokio lock, `.len()` as the
  bound instead of `.capacity()`).
- **setup** — all of the above, plus: `[package.metadata.trust]` present in
  Cargo.toml, no per-file `#![strict]` markers, and plain `cargo build`
  still **fails** — i.e. the agent did not "fix" the stock-rustc failure by
  stripping the named arguments.
- **authoring** — the existing shipped/caught methodology (bug regex in
  source, `trust check` catch, shipped = bug ∧ ¬caught).

Each fixture directory contains a `reference-solution/` crate — a
hand-written passing solution used to validate the predicates. It is
**never shown to the agent** (`run_agent_eval.py` excludes it when copying
the fixture into the work directory).

## Running the suite

One-time toolchain build (the runner and scorer refuse to start without it,
mirroring `../score.py`'s RT-53 guard):

```sh
cargo build -p trust-lang -p cargo-trustc -p trust-rustc -p trust-rustdoc
```

API keys: `OPENAI_API_KEY` for `--provider openai`, `GOOGLE_API_KEY` for
`--provider gemini` (`pip install openai` / `pip install google-genai`).
Provider adapters are shared with `../run_cross_provider.py`. There is no
Anthropic HTTP adapter on purpose — run the Claude arm by spawning Haiku
subagents from a Claude session into the same output layout, as in runs
001–004.

Run ids follow the existing `eval/runs/<NNN>/` numbering — take the next
free number (007 as of this writing) and use **one run id per
provider+model per suite**; don't mix agent-workflow work directories and
single-file `.rs` outputs in one run dir (different scorers).

### Workflow tasks (setup + remediation), one command per arm

```sh
python3 eval/agent-workflow/run_agent_eval.py --provider openai --model gpt-4o --run 007 --arm bare
python3 eval/agent-workflow/run_agent_eval.py --provider openai --model gpt-4o --run 007 --arm skilled

python3 eval/agent-workflow/score_agent.py 007
python3 eval/agent-workflow/summarize_agent.py 007
cat eval/runs/007/summary.md
```

Outputs: `eval/runs/007/<task>-<arm>-<trial>/` (final work tree +
`transcript.json`), `<task>-<arm>-<trial>.score.json`, `summary.md` with a
per-task table and the bare→skilled pass-rate delta.

### Authoring tasks, one command per arm

The skilled arm uses the runner's `--context-file` flag (added for RT-97);
each arm gets its own run id:

```sh
python3 eval/run_cross_provider.py --provider openai --model gpt-4o --run 008 \
    --tasks 16-frame-parse,17-async-cache --trials 3
python3 eval/run_cross_provider.py --provider openai --model gpt-4o --run 009 \
    --tasks 16-frame-parse,17-async-cache --trials 3 \
    --context-file skills/writing-trust/SKILL.md

python3 eval/score.py 008 && python3 eval/summarize.py 008
python3 eval/score.py 009 && python3 eval/summarize.py 009
```

## Cost ballpark (per model, both arms)

- Workflow: 5 tasks × 2 arms × 3 trials = 30 sessions, ≤3 rounds each →
  ≤90 calls. Worst-case ≈8k tokens in / 1k out per call (skill text +
  fixture + replayed history) → ≈0.6M in / 0.09M out.
  gpt-4o ($2.50/M in, $10/M out): ≈ **$2.40 worst case**, ~$1 typical
  (most trials finish in 1–2 rounds). gemini-2.5-flash: ≈ $0.40.
- Authoring: 2 tasks × 2 conditions × 3 trials × 2 arms = 24 calls, small
  prompts → ≈ **$0.25** on gpt-4o.

Total: under **$3 per model** on gpt-4o-class pricing; both suites on both
cross providers comfortably under $5. Claude-arm cost depends on the
session driving it.

## Fixture validation (done at authoring time, no API)

Recorded here so a future rules change that breaks a fixture is noticed —
re-run these checks after touching trust-lints (clear `$TMPDIR/trust-cache`
and `/tmp/trust-cache` between builds; grep R-codes with `R[0-9]{4}`):

| Fixture | Broken state | Reference solution |
|---------|--------------|--------------------|
| `30-setup-logstat` | plain `cargo build` green (exit 0) | `cargo trustc build` green; plain `cargo build` fails (exit 101) — named args kept |
| `31-remediate-r0018` | `cargo trustc build` fails, exactly 1×R0018 | green |
| `32-remediate-r0019` | fails, exactly 1×R0019 | green |
| `33-remediate-r0020` | fails, exactly 1×R0020 | green |
| `34-remediate-r0021` | fails, exactly 1×R0021 | green |

Every `bug`/`good` regex was checked both ways (bug matches broken / not
reference; good matches reference / not broken), the setup extras
(metadata key, marker, plain-cargo) likewise, and the authoring regexes
were checked against hand-written tempted and fixed solutions (`trust
check` confirms R0019/R0020 fire on the tempted versions). The scorer +
summarizer were dry-run end to end on a synthetic run (broken fixtures as
the bare arm — 0/5 pass; reference solutions as the skilled arm — 5/5
pass).

Known scope notes: R0020's anti-pattern is scope-sensitive, so task 33/17's
bug regex carries its own fix-idiom exceptions and the rule re-run stays
authoritative; `17-async-cache` deliberately uses a free `async fn` because
the R0020 visitor currently skips `impl` methods.
