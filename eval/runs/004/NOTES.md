# Run 004 — Sonnet replication of run 002

## Headline

**Sonnet matches Haiku exactly on the headline numbers.**

| | Vanilla bug-ship | Rustricted bug-ship | Reduction |
|---|---|---|---|
| Haiku (run 002) | 9/15 (60%) | 0/15 (0%) | 100% |
| Sonnet (run 004) | 9/15 (60%) | 0/15 (0%) | 100% |

The dialect catches 100% of the bugs both models ship in vanilla mode. The expected pattern — "stronger model ships fewer bugs in vanilla" — did NOT materialize for this task suite. Both models bug at the same rate on the same tasks.

## Per-task comparison

| Task | Vanilla (Haiku → Sonnet) | Rustricted bug-in-source | Caught |
|------|--------------------------|--------------------------|--------|
| 11-pipeline | 3/3 → 3/3 | 3/3 → 3/3 | both: 3/3 |
| 12-result-chain | 0/3 → 0/3 | 0/3 → 0/3 | — |
| 13-numeric | 3/3 → 3/3 | 3/3 → 3/3 | both: 3/3 |
| 14-imports | 0/3 → 0/3 | 0/3 → 0/3 | — |
| 15-many-points | 3/3 → 3/3 | 3/3 → 2/3 | both: 3/3 |

The only cell that differs: **15-many-points-rustricted-2** under Sonnet. Sonnet actually wrote `make_point(x: 0, y: 0, z: 0)` — real named-argument syntax it presumably extrapolated from "`#![strict]` enables additional checks." Haiku never produced named-arg syntax in any rustricted trial. The Sonnet attempt is rare (1 of 15 rustricted outputs) but exists.

## Behavioral observation

Across all 15 Sonnet rustricted outputs, only one (15-many-points-rustricted-2) showed dialect-aware behavior. The rest were byte-similar to their vanilla counterparts apart from `#![strict]`. So Sonnet exhibits a weak version of dialect inference that Haiku does not, but it's noisy enough that the *bug-ship* rate doesn't move. The dialect catches the bug either way.

This actually strengthens the project's main pitch: **the dialect's catch-rate doesn't depend on whether the agent infers the dialect's semantics from the prompt.** Whether the agent writes vanilla code or makes a half-hearted attempt at the dialect, `rustricted check` produces the same outcome — bugs don't ship.

## What this tells us

1. **R0042 (positional ordering) is the dominant value.** It fires on tasks 11 and 15 every single trial across both models. That's 6 prevented bugs per condition cycle.
2. **R0003 (`as` cast) is reliable in vanilla.** Both models reach for `as u32` / `as u64` on numeric helpers, and the dialect catches every time.
3. **R0001 (.unwrap), R0004 (glob)** don't fire because neither model reaches for those reflexes on the eval tasks — same as Haiku in runs 002 and 003.
4. **Stronger model ≠ fewer bugs on this task suite.** Both Haiku and Sonnet produce the same vanilla code (modulo whitespace and variable naming). The bug-class default is set by training-distribution familiarity, not by model strength on these single-shot, well-scaffolded tasks.

## Limitations

1. **n=3 per cell, Anthropic-only.** Cross-provider replication (GPT, Gemini) would test whether the pattern is Anthropic-specific or broadly true.
2. **Single-shot single-file tasks.** Real production code is iterative and multi-file. The dialect's value on those scenarios is implied but not measured here.
3. **The 15-many-points-rustricted-2 named-args attempt** is one observation. To test whether stronger models can be nudged into using the dialect's syntax via the blind prompt alone, we'd need many more trials with different prompt wordings.
4. **The "Sonnet writes the same code as Haiku" finding is itself one trial × 5 tasks deep.** Could be task-suite-specific.

## What to do next

Real-world conversion is the obvious next step — the eval has saturated useful signal at this scale. Run a real Rust crate through `rustricted check` with `#![strict]` toggled on (no, wait: the dogfood task from earlier showed that requires a `rustricted-attrs` proc-macro shim first). So:

1. **Build `rustricted-attrs` proc-macro** that makes `#![strict]` a no-op for rustc. Then real cargo crates can opt in.
2. **Then** redo the dogfood task on `rustricted-syntax`.
3. **Then** try a small external crate (anyhow or similar — see eval/false-positives/REPORT.md for the FP profile we'd expect).

Or, if more eval is wanted: cross-provider replication is the highest-information move. Same tasks, GPT-4 / Gemini, see if the 60% vanilla rate holds.
