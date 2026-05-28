# Run 003 — interpretation

## Headline

Vanilla shipped 2/6 (33%) bugs. Trust shipped 0/6 (0%).

Two new tasks designed to fill the gap from run 002 (where tasks 12 and 14 elicited no bugs):

- **22-result-chain (R0001 .unwrap)**: 6-step inline pipeline in `fn main() { ... }`. No helper functions, no `?` propagation — main returns `()`. Designed to make `.unwrap()` the path of least resistance.
- **24-imports (R0004 glob)**: 16 distinct types named across `std::collections`, `std::io`, `std::path`, `std::sync`. Designed to make glob imports tempting.

## What happened

### Task 22 — partial win

- **Vanilla**: 2/3 trials shipped `.unwrap()` chained over `read_to_string`, `lines().next()`, and `parse()`. 1/3 trials used `unwrap_or_default()` / `if let Ok(...)` — graceful by accident.
- **Trust**: 0/3 had `.unwrap()`. All three used `match` blocks with `eprintln!` + early `return`.

Interesting wrinkle: the redesigned task did elicit R0001 bugs in vanilla, but the blind trust prompt produced bug-free code without the dialect needing to catch anything. Different from run 002, where Haiku wrote byte-identical code in both conditions. Hypothesis: the trust prompt's mention of "additional set of static checks" nudges Haiku toward defensive patterns when the task explicitly involves error-prone steps.

This is real signal but weaker than run 002's "100% catch" result. The dialect's value here is mixed: it doesn't have to catch anything because the agent doesn't write the bug, but the agent only declined to write the bug *because the prompt mentioned strict checks*. Whether that nudge generalizes outside the eval context is an open question.

### Task 24 — no signal

All 6 trials (both conditions) used explicit `use std::collections::{HashMap, HashSet, BTreeMap, BTreeSet, VecDeque}; use std::io::{...}; ...`. Even with 16 type names spread across 4 modules, Haiku reliably writes explicit imports. The R0004 bug class doesn't exist for Haiku at this scale.

To elicit a glob from Haiku, you'd probably need either:
- A prompt explicitly inviting the glob (e.g. "minimize import lines"), which would be biased.
- A vastly larger surface (30+ types across many modules) where typing them all becomes obviously tedious.
- A different model that defaults to globs in similar situations.

For now: task 24 is a no-op. Either remove it from the eval suite or accept that R0004 is rarely a real concern for current-generation agents.

## Combined with runs 001+002

After three runs the picture is:

| Bug class | Rule | Vanilla bug rate | Dialect catches? |
|-----------|------|------------------|------------------|
| Positional args | R0042 | high (6/6 across runs) | yes, always |
| `as` integer cast | R0003 | high (3/3 in run 002) | yes, always |
| `.unwrap()` reflex | R0001 | medium / task-dependent (2/3 in run 003 task 22; 0/3 in older tasks where prompt scaffolds error handling) | yes when bug present |
| Glob imports | R0004 | zero | n/a — Haiku never globs at the eval scale |
| `panic!()`, `todo!()`, `as` in non-numeric contexts | R0011, R0010, others | not measured | n/a |

The strongest evidence is for R0042 — and that's also the rule the project's whole pitch was built around. The other rules are either reliable (R0003) but smaller surface, or task-dependent (R0001), or not exercised in practice (R0004).

## Limitations

1. **n=3 per cell, Haiku only.** Same as run 002.
2. **Task 24 produced no signal.** The eval should not pretend "0 bugs / 0 caught" is evidence — it's just absence of evidence.
3. **The "nudge" observation in task 22 is unreplicated.** Run 002 showed no nudge; run 003 task 22 shows one. Could be variance, could be task-shape-dependent.

## Next

The most informative next run would be **cross-model replication on Sonnet** — same task suite, see if a stronger model still ships bugs vanilla (predicted: at lower rate) and whether the dialect still catches whatever it does ship. That's run 004.
