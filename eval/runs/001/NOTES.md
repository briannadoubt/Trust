# Run 001 — interpretation

## Headline

Haiku shipped bugs in 2/5 vanilla tasks (40%), 0/5 trust tasks (0%). The 100% bug-ship reduction is real but small-n (1 trial per cell) and the bug surface was narrower than designed.

## What actually happened, task by task

- **01-duration**: vanilla wrote `make_duration(60, 500)` (positional, R0042 bug). Trust wrote `make_duration(secs: 60, nanos: 500)` (named) because the prompt explicitly told it to. ✓ Signal as designed.
- **02-area**: same pattern. Vanilla shipped `rect_area(1920, 1080)`. Trust complied with named args. ✓ Signal as designed.
- **03-config**: vanilla used `?` correctly. No bug to ship; nothing for R0001 to catch. The scaffold ("the function must propagate IO errors using `?`") was too leading.
- **04-cast**: vanilla used `u32::try_from`. No bug. The scaffold demanded a `Result<u32, TryFromIntError>` return type, which forced the agent away from `as u32`.
- **05-glob**: vanilla used `use std::collections::{HashMap, HashSet, BTreeMap, BTreeSet}`. No bug. The task said "pick whatever import style is best" — and Haiku correctly picked explicit imports.

## What this means

Run 001 confirms one thing well: **when the agent doesn't know to use named args (vanilla prompt), it ships positional ordering by default.** The dialect's named-args feature genuinely closes that hole — but only because the trust prompt explicitly mentioned it.

Run 001 fails to test the more interesting question: **when the agent does make a bug despite being in Trust mode, does `trust check` catch it before deploy?** None of these tasks elicited a Trust-mode bug, so `trust check` had nothing to catch (the "caught" column is all ✗ only because "bug in source" was all 0).

## Limitations

1. **n=1 per cell.** Single trials are too noisy to make claims past direction-of-effect.
2. **Three of five tasks are too easy for vanilla.** Haiku writes correct Rust for `?`, `try_from`, and explicit imports unprompted. The bug surface needs to be wider or more distracted.
3. **The trust prompts give away the rule.** "In strict mode, calls to local fns with > 1 arg MUST use named args" — that's the rule itself. A fair test would mention only that `#![strict]` is enabled and let the agent infer (or fail).
4. **No measure of `trust check` catching real bugs**, because no real Trust-mode bugs were made.

## Next run

Run 002 should:

- Increase trials to 5 per cell (50 agent runs total).
- Redesign tasks 3, 4, 5 to be longer/noisier so Haiku has more places to slip. E.g. task 3 includes a parser with multiple `Result`-returning calls where forgetting `?` once is plausible.
- Add a "blind" trust prompt that says only "use Trust, file starts with `#![strict]`" without enumerating each rule. This tests whether familiarity with the dialect (rather than direct instruction) reduces bugs.
- Add at least one task that uses `trust-std` shims so R0042 can fire on stdlib-equivalent calls.
