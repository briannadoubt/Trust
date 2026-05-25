//! Catalogue of diagnostic codes emitted by lowering passes.
//!
//! Each pass references `Rule::X.code()` at its emission site instead of
//! using a raw string literal. This makes typos a compile error and gives
//! `cargo xtask gen-docs` a single source of truth for the non-strict
//! diagnostics table in `docs/SPEC.md`.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rule {
    /// R2001 — RHS of `|>` was not a callable path.
    PipeRhsNotPathCall,
    /// R3001 — named argument does not match any declared parameter.
    NamedArgUnknownParam,
}

impl Rule {
    pub fn code(self) -> &'static str {
        match self {
            Rule::PipeRhsNotPathCall => "R2001",
            Rule::NamedArgUnknownParam => "R3001",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Rule::PipeRhsNotPathCall => "pipe-rhs-not-path-call",
            Rule::NamedArgUnknownParam => "named-arg-unknown-param",
        }
    }

    pub fn pass(self) -> &'static str {
        match self {
            Rule::PipeRhsNotPathCall => "pipe lowering",
            Rule::NamedArgUnknownParam => "named-args lowering",
        }
    }

    pub fn message_shape(self) -> &'static str {
        match self {
            Rule::PipeRhsNotPathCall => "pipe `|>` requires a path-call on the right",
            Rule::NamedArgUnknownParam => "`{fn}` has no parameter named `{arg}`",
        }
    }
}

pub const ALL: &[Rule] = &[Rule::PipeRhsNotPathCall, Rule::NamedArgUnknownParam];
