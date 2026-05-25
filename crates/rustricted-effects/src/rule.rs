//! Catalogue of diagnostic codes emitted by the effects checker.
//!
//! Mirrors the pattern in `rustricted-lower::rule`: each emission site
//! references `Rule::X.code()`, and `cargo xtask gen-docs` reads the
//! catalogue to render the non-strict diagnostics table in SPEC.md.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rule {
    /// R4001 — declared effect set does not cover the inferred effects.
    MissingDeclaredEffect,
}

impl Rule {
    pub fn code(self) -> &'static str {
        match self {
            Rule::MissingDeclaredEffect => "R4001",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Rule::MissingDeclaredEffect => "missing-declared-effect",
        }
    }

    pub fn pass(self) -> &'static str {
        match self {
            Rule::MissingDeclaredEffect => "effects check",
        }
    }

    pub fn message_shape(self) -> &'static str {
        match self {
            Rule::MissingDeclaredEffect => "`{fn}` is missing declared effect(s): {effects}",
        }
    }
}

pub const ALL: &[Rule] = &[Rule::MissingDeclaredEffect];
