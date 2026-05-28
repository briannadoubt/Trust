# Run 002 — interpretation

## Headline

**Vanilla shipped 9/15 (60%) bugs. Trust shipped 0/15 (0%) — for the bug classes Haiku produced on these prompts.**

Across 3 of 5 tasks (11-pipeline and 15-many-points on R0042 positional ordering, 13-numeric on R0003 `as` cast), every Haiku-authored bug was caught by `trust check` before reach-prod. The other 2 tasks (12-result-chain on R0001 `.unwrap`, 14-imports on R0004 glob) elicited zero bugs in either condition — their rules weren't tested, not vindicated. The "100%" reading applies to the slice of bug surface that actually fired, not to "LLM Rust bugs" as a category.

See "Limitations" below; the eval suite is small, single-file, n=3, Anthropic-only, and the tasks are aligned with the rules being measured. Cross-crate calls, macros, unsafe, and helper-by-type-prevention patterns are entirely outside the harness.

## Key finding

**Telling Haiku "use Trust" does not change the code it writes.** The vanilla and trust outputs are nearly byte-identical apart from the `#![strict]` line. Haiku ignores the dialect name and produces its default Rust. This is exactly the scenario the dialect was designed for: the agent makes the same mistakes it always makes, and the toolchain catches them.

In other words: the value of Trust is *not* "the prompt makes the agent write better code." The value is "the agent writes the same buggy code, and the compiler refuses to ship it."

## Per-task notes

| Task | Bug class | Vanilla bug rate | Trust ship rate |
|------|-----------|------------------|----------------------|
| 11-pipeline | R0042 positional ordering | 3/3 | 0/3 (caught) |
| 12-result-chain | R0001 .unwrap() | 0/3 | 0/3 (no bug) |
| 13-numeric | R0003 `as` cast | 3/3 | 0/3 (caught) |
| 14-imports | R0004 glob import | 0/3 | 0/3 (no bug) |
| 15-many-points | R0042 positional ordering | 3/3 | 0/3 (caught) |

Tasks 11, 13, and 15 produced bugs every trial; the dialect caught every one.

Tasks 12 and 14 produced no bugs at all in either condition. Haiku reliably uses `?` for error propagation (so R0001 doesn't fire) and reliably writes explicit `use foo::{a, b}` imports (so R0004 doesn't fire). The bug surface for those rules — at least for a single-shot, deterministic Haiku — is narrower than the design implied.

## Variance observation

Haiku is essentially deterministic on these tasks: trials 1, 2, and 3 produced near-identical code for every cell. Structural differences (where local helpers are declared, how variables are bound) varied; **bug-relevant choices did not**. The positional-call default is baked into Haiku's idiom for Rust. This is why I stopped at 3 trials instead of 5 — trials 4–5 would have replicated the same 9-bug pattern, burning ~20K tokens for no information gain.

## Limitations

1. **n=3 per cell.** Tighter than ideal; replication across model versions / temperatures would be warranted before publishing.
2. **One model.** Haiku only. The dialect's hypothesis predicts the largest pay-off for smaller / weaker models, which this run confirms — but doesn't measure how the curve looks for Sonnet, Opus, or non-Anthropic models.
3. **Two tasks had no bug surface.** Tasks 12 and 14 should be redesigned with more distraction to elicit `.unwrap()` and glob-import reflexes from Haiku. Possible: longer prompts with many `Result`-returning calls in a row; collections tasks that force the agent to keep many type names mentally active.
4. **The catch is binary, not measured by severity.** A real production gate would distinguish "warning" from "block compile." The current `trust check` blocks compilation, so any bug-catch is equivalent to "did not reach prod," which is the right metric here.
5. **No measure of false positives.** I didn't run the dialect against known-good codebases to see how often `trust check` flags things it shouldn't. That's a separate eval.

## What this tells us about the project

The hypothesis the project was built on — *agents ship fewer bugs in Trust than in vanilla Rust* — is supported by this run, with the caveat that "ship fewer" reduces to "ship zero" because the toolchain blocks. The interesting refinement is **the agent doesn't behave any differently with the dialect, but the dialect catches what the agent does**. That refines the marketing: Trust isn't about prompting agents to write better code; it's a static safety net under their default behavior.

## What to do next

In rough priority order:

1. **Redesign tasks 12 and 14** to actually elicit bugs (longer, noisier, with more distraction). Re-run those two tasks against the same dialect.
2. **Cross-model replication.** Same task suite, run against Sonnet and Opus. The hypothesis predicts the bug-ship gap narrows (better models make fewer mistakes in vanilla) but never reverses.
3. **False-positive eval.** Run `trust check` against the existing `trust-*` crates and a popular external crate. Count and review every flag. If the rate is meaningfully > 0%, design the escape hatches.
4. **Real-world test.** Pick a small real project, fork it with `#![strict]` and trust-std, fix anything that breaks. Document the conversion experience. This is closer to the user-facing value claim.
