# Agent instructions for Trust projects

Paste the block below into your project's `CLAUDE.md` or `AGENTS.md` so any
coding agent gets the essentials of working in a Trust codebase. Projects
scaffolded with `trust new` already include it as `CLAUDE.md`.

---

## Trust (strict Rust dialect)

This project uses Trust, a strict Rust dialect that lowers to plain Rust.
Build, run, and test with `cargo trust build|run|test` — NEVER plain `cargo`:
named-argument syntax won't parse under stock cargo, and that's expected.

- Calls with more than one argument use named arguments:
  `make_rect(width: 1920, height: 1080)`. Also available: the pipe operator
  `e |> f(args)` and `requires!(cond)` preconditions.
- Build errors with R-codes (R0001, R0042, …) are Trust teaching errors:
  read the `why:` and `help:`/`instead:` text, apply it, then rebuild.
  - `trust explain <CODE>` — detail on one rule.
  - `trust fix <file> --write` — auto-inserts argument names.
  - `cargo trust build --message-format json` — machine-readable diagnostics.
- `#[cfg(test)]` code is exempt — write tests in plain Rust; don't convert them.
- Don't suppress rules without a `reason`. Suppression via
  `#[allow(trust::R0xxx, reason = "…")]` only compiles under cargo trust,
  never stock cargo.

Docs: https://github.com/briannadoubt/Trust — see `docs/WRITING-TRUST.md`
for the full agent guide.
