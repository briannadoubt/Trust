//! Rule registry. Each entry is a stable code (`R0001`, `R0002`, …), a short
//! name, and a one-sentence rationale. The runner dispatches by `Rule`.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rule {
    /// `.unwrap()` used outside `#[cfg(test)]`.
    NoUnwrap,
    /// `.expect("")` with an empty message.
    EmptyExpect,
    /// `as` cast used.
    NoAsCast,
    /// `use foo::*` glob import.
    NoGlobImport,
    /// `unsafe` block without `// safety:` comment.
    JustifyUnsafe,
    /// `#[allow(...)]` without `// reason:` comment.
    JustifyAllow,
    /// `impl Trait` return type without a named type alias.
    NoImplTraitReturn,
    /// User-defined `macro_rules!` without `#[strict::macros_ok]` opt-in.
    NoUserMacros,
    /// Positional argument to a locally-defined function with arity > 1.
    /// Emission lives in `rustricted-lower::named_args` because the lint
    /// must fire before lowering strips argument names; this catalogue
    /// entry exists so SPEC.md, docs, and tooling can refer to R0042.
    NoPositionalArgs,
}

impl Rule {
    pub fn code(self) -> &'static str {
        match self {
            Rule::NoUnwrap => "R0001",
            Rule::EmptyExpect => "R0002",
            Rule::NoAsCast => "R0003",
            Rule::NoGlobImport => "R0004",
            Rule::JustifyUnsafe => "R0005",
            Rule::JustifyAllow => "R0006",
            Rule::NoImplTraitReturn => "R0007",
            Rule::NoUserMacros => "R0008",
            Rule::NoPositionalArgs => "R0042",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Rule::NoUnwrap => "no-unwrap",
            Rule::EmptyExpect => "empty-expect",
            Rule::NoAsCast => "no-as-cast",
            Rule::NoGlobImport => "no-glob-import",
            Rule::JustifyUnsafe => "justify-unsafe",
            Rule::JustifyAllow => "justify-allow",
            Rule::NoImplTraitReturn => "no-impl-trait-return",
            Rule::NoUserMacros => "no-user-macros",
            Rule::NoPositionalArgs => "no-positional-args",
        }
    }

    pub fn rationale(self) -> &'static str {
        match self {
            Rule::NoUnwrap => "panics on None/Err are silent control flow; agents reach for `.unwrap()` reflexively",
            Rule::EmptyExpect => "empty messages defeat the point of `.expect()`",
            Rule::NoAsCast => "`as` silently truncates and is a frequent source of integer-overflow bugs",
            Rule::NoGlobImport => "glob imports hide which symbols are in scope and let unrelated changes affect resolution",
            Rule::JustifyUnsafe => "every `unsafe` block must explain the invariant being upheld",
            Rule::JustifyAllow => "every `#[allow]` must explain why the rule is being suppressed",
            Rule::NoImplTraitReturn => "anonymous return types kill local reasoning; name the type with an alias",
            Rule::NoUserMacros => "macros expand non-locally; agents misuse them frequently",
            Rule::NoPositionalArgs => "positional argument ordering is the largest LLM-authored bug class in Rust; named args eliminate it",
        }
    }
}

pub const ALL: &[Rule] = &[
    Rule::NoUnwrap,
    Rule::EmptyExpect,
    Rule::NoAsCast,
    Rule::NoGlobImport,
    Rule::JustifyUnsafe,
    Rule::JustifyAllow,
    Rule::NoImplTraitReturn,
    Rule::NoUserMacros,
    Rule::NoPositionalArgs,
];
