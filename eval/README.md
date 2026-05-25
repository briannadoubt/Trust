# Rustricted eval

Measures the project's central hypothesis: *agents ship fewer bugs in Rustricted than in vanilla Rust*.

## Method

Each task is run twice through a fresh Haiku subagent — once with a vanilla-Rust prompt, once with a Rustricted prompt. The agent's raw output is saved as a `.rs` file. We then score:

1. **Bug in source** — regex match against a known-bad pattern (e.g. positional `make_duration(60, 500)`).
2. **Dialect caught** (Rustricted condition only) — does `rustricted check` report the expected rule code?
3. **Shipped** — bug present *and* dialect didn't catch it. This is the headline metric: how often does a bug make it past the toolchain to production?

Tasks are deliberately tiny and self-contained — single-file programs whose bug surface is one of R0001 (.unwrap), R0003 (as cast), R0004 (glob import), or R0042 (positional args).

Haiku is the deliberate choice. Larger models make fewer of these bugs in vanilla Rust, which compresses the signal. The dialect should pay off most where the author makes the most mistakes.

## Layout

```
tasks.toml              # all tasks: prompts, bug patterns, expected rule
runs/<NNN>/             # one directory per eval run
  <task>-<cond>.rs        # raw agent output
  <task>-<cond>.score.json  # per-trial score
  summary.md              # aggregated table
score.py                # produces .score.json files for a run
summarize.py            # writes summary.md for a run
```

## Running

Spawning the subagents is done from a Claude session (see commit log for examples). Once outputs are in `runs/<NNN>/`:

```sh
python3 eval/score.py 001
python3 eval/summarize.py 001
cat eval/runs/001/summary.md
```
