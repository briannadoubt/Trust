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
    /// `todo!()` or `unimplemented!()` shipped in non-test code.
    NoTodoMacro,
    /// `panic!()` shipped in non-test code.
    NoPanic,
    /// Visible function signature uses `bool` as a parameter type.
    NoBoolParam,
    /// `expr[idx]` indexing where `idx` is not a literal integer.
    NoBareIndex,
    /// Visible function with two adjacent parameters of the same concrete
    /// type (silently swappable; wrap each in a distinct newtype).
    NoSameTypeParams,
    /// `#[allow(trust::Rxxxx)]` missing `reason = "..."` argument.
    AllowMissingReason,
    /// `#[allow(trust::Rxxxx)]` with an unknown rule code.
    AllowUnknownCode,
    /// `.map_err(|_| …)` or `.ok().expect(…)` discarding the source error.
    NoErrorContextDrop,
    /// Bare `+`/`-`/`*` where an operand is a `.len()` call — debug panics,
    /// release silently wraps.
    NoUncheckedLenArith,
    /// A sync lock guard (`.lock().unwrap()` et al) bound before an
    /// `.await` in the same async block.
    NoLockAcrossAwait,
    /// `.capacity()` used as an index or range bound where `.len()` is
    /// almost certainly meant.
    NoCapacityAsLen,
    /// Positional argument to a locally-defined function with arity > 1.
    /// Emission lives in `trust-lower::named_args` because the lint
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
            Rule::NoTodoMacro => "R0010",
            Rule::NoPanic => "R0011",
            Rule::NoBoolParam => "R0012",
            Rule::NoBareIndex => "R0014",
            Rule::NoSameTypeParams => "R0017",
            Rule::AllowMissingReason => "R0015",
            Rule::AllowUnknownCode => "R0016",
            Rule::NoErrorContextDrop => "R0018",
            Rule::NoUncheckedLenArith => "R0019",
            Rule::NoLockAcrossAwait => "R0020",
            Rule::NoCapacityAsLen => "R0021",
            Rule::NoPositionalArgs => "R0042",
        }
    }

    /// Parse a rule code like `R0014` back to a `Rule`. Returns `None` for
    /// unknown codes — `#[allow(trust::Rxxxx)]` callers must validate.
    pub fn from_code(code: &str) -> Option<Self> {
        ALL.iter().copied().find(|r| r.code() == code)
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
            Rule::NoTodoMacro => "no-todo-macro",
            Rule::NoPanic => "no-panic",
            Rule::NoBoolParam => "no-bool-param",
            Rule::NoBareIndex => "no-bare-index",
            Rule::NoSameTypeParams => "no-same-type-params",
            Rule::AllowMissingReason => "allow-missing-reason",
            Rule::AllowUnknownCode => "allow-unknown-code",
            Rule::NoErrorContextDrop => "error-context-dropped",
            Rule::NoUncheckedLenArith => "no-unchecked-len-arith",
            Rule::NoLockAcrossAwait => "no-lock-across-await",
            Rule::NoCapacityAsLen => "no-capacity-as-len",
            Rule::NoPositionalArgs => "no-positional-args",
        }
    }

    /// `false` for catalogue entries reserved for future implementation —
    /// `cargo xtask check-emissions` skips these. Set back to `true` once
    /// a real emission site lands.
    pub fn is_implemented(self) -> bool {
        // All currently-catalogued rules are implemented. New variants
        // reserved for future work should return false here.
        true
    }

    /// `true` if this rule's visitor walks `#[cfg(test)]` / `#[test]`
    /// scopes and silences itself inside them. The asymmetry across
    /// rules is intentional but historically lived only in visitor
    /// source — this method makes it discoverable and renderable in
    /// the docs catalogue.
    pub fn is_exempt_in_cfg_test(self) -> bool {
        matches!(
            self,
            Rule::NoUnwrap
                | Rule::NoErrorContextDrop
                | Rule::NoUncheckedLenArith
                | Rule::NoLockAcrossAwait
                | Rule::NoCapacityAsLen
                | Rule::NoGlobImport
                | Rule::NoUserMacros
                | Rule::NoTodoMacro
                | Rule::NoPanic
                | Rule::NoBoolParam
                | Rule::NoBareIndex
                | Rule::NoSameTypeParams
        )
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
            Rule::NoTodoMacro => "`todo!()` / `unimplemented!()` ship as runtime panics; finish or fence behind `cfg(test)`",
            Rule::NoPanic => "explicit panics drop typed errors on the floor; return `Err` and let the caller decide",
            Rule::NoBoolParam => "raw `bool` parameters are positional footguns; named enums make intent self-documenting",
            Rule::NoBareIndex => "`v[i]` panics on out-of-bounds; `.get(i)` makes the failure path explicit",
            Rule::NoSameTypeParams => "adjacent same-type parameters are silently swappable; named args fix the call site but not values built into the wrong variable — distinct newtypes make a swap a type error",
            Rule::AllowMissingReason => "every `#[allow(trust::Rxxxx)]` must include a `reason = \"...\"` justification",
            Rule::AllowUnknownCode => "`#[allow(trust::Rxxxx)]` references a rule code that is not in the registry",
            Rule::NoErrorContextDrop => "`.map_err(|_| …)` and `.ok().expect(…)` discard the source error agents need to debug the failure; the chain is the context",
            Rule::NoUncheckedLenArith => "bare arithmetic on `.len()`-derived values panics in debug and silently wraps in release; the underflow of `len() - 1` on empty input is a classic agent bug",
            Rule::NoLockAcrossAwait => "holding a sync `MutexGuard` across `.await` blocks every task on that lock and can deadlock single-threaded runtimes",
            Rule::NoCapacityAsLen => "`.capacity()` is allocation size, not element count; using it as a bound reads uninitialized slots or panics",
            Rule::NoPositionalArgs => "positional argument ordering is the largest LLM-authored bug class in Rust; named args eliminate it",
        }
    }

    /// The canonical compliant idiom — what to write *instead*, in one line.
    /// This is the agent-actionable counterpart to [`rationale`](Self::rationale)
    /// (which says *why*). Surfaced by `trust explain` and the generated
    /// `docs/WRITING-TRUST.md` agent guide (RT-78).
    pub fn instead(self) -> &'static str {
        match self {
            Rule::NoUnwrap => "propagate with `?`, or `.expect(\"why this can't fail\")`",
            Rule::EmptyExpect => {
                "give `.expect(\"…\")` a real message explaining why it can't fail"
            }
            Rule::NoAsCast => "use `T::try_from(x)?` for fallible casts, or `.into()` for widening",
            Rule::NoGlobImport => "import the specific items: `use foo::{A, B};`",
            Rule::JustifyUnsafe => "precede the `unsafe` block with a `// safety: …` comment",
            Rule::JustifyAllow => "precede the `#[allow(…)]` with a `// reason: …` comment",
            Rule::NoImplTraitReturn => {
                "name the type with a `type Alias = …;` and return the alias"
            }
            Rule::NoUserMacros => "inline the logic, or opt in with `#[strict::macros_ok]`",
            Rule::NoTodoMacro => "finish the implementation, or return a typed `Err`",
            Rule::NoPanic => "return a typed `Err` and let the caller decide whether to abort",
            Rule::NoBoolParam => {
                "replace the `bool` with a named enum, e.g. `enum Mode { On, Off }`"
            }
            Rule::NoBareIndex => "use `.get(i)` and handle the `Option`",
            Rule::NoSameTypeParams => {
                "wrap each in a distinct newtype — `trust_std::newtype!(pub Width(u32));`"
            }
            Rule::AllowMissingReason => {
                "add a `reason = \"…\"` argument to the `#[allow(trust::…)]`"
            }
            Rule::AllowUnknownCode => {
                "use a real rule code (run `trust explain` for the catalogue)"
            }
            Rule::NoErrorContextDrop => {
                "carry the source: `.map_err(|e| MyError::Io(e))`, or use `?` with a `From` impl"
            }
            Rule::NoUncheckedLenArith => {
                "make the choice explicit: `.checked_sub(1)?`, `.saturating_sub(1)`, or `.wrapping_*` if wrap is intended"
            }
            Rule::NoLockAcrossAwait => {
                "drop the guard before awaiting (scope it in a block), or use an async-aware lock like `tokio::sync::Mutex`"
            }
            Rule::NoCapacityAsLen => {
                "use `.len()` for element counts; `.capacity()` only sizes future allocations"
            }
            Rule::NoPositionalArgs => {
                "name the arguments — `f(width: …, height: …)` — or run `trust fix`"
            }
        }
    }
}

pub const ALL: &[Rule] = &[
    Rule::NoUnwrap,
    Rule::NoErrorContextDrop,
    Rule::NoUncheckedLenArith,
    Rule::NoLockAcrossAwait,
    Rule::NoCapacityAsLen,
    Rule::EmptyExpect,
    Rule::NoAsCast,
    Rule::NoGlobImport,
    Rule::JustifyUnsafe,
    Rule::JustifyAllow,
    Rule::NoImplTraitReturn,
    Rule::NoUserMacros,
    Rule::NoTodoMacro,
    Rule::NoPanic,
    Rule::NoBoolParam,
    Rule::NoBareIndex,
    Rule::NoSameTypeParams,
    Rule::AllowMissingReason,
    Rule::AllowUnknownCode,
    Rule::NoPositionalArgs,
];
