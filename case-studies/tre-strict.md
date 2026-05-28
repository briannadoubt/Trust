# Case Study: `tre` v0.4.0 under Trust strict mode

**Crate:** [tre](https://github.com/dduan/tre) v0.4.0 (commit `2caab28`)
**License:** MIT
**Original LOC:** 1,080 (8 source files)
**Strict mode activated via:** `trust_attrs::strict!{}` in each `.rs` file
**Build method:** `RUSTC_WRAPPER=$(realpath target/debug/trust-rustc) cargo build`
**Final build result:** ✅ Clean build + 4/4 unit tests passing
**Case study path:** `case-studies/tre-strict/`

---

## What `tre` does

`tre` is a `tree(1)` replacement written in Rust. It walks the filesystem,
formats the result as either a Unicode tree diagram or a JSON document, and
can optionally emit shell aliases for `$EDITOR`-opening the listed entries.

It was chosen for the second case study because the first (heck v0.5.0) was
a deliberately pure library — no I/O, no error paths, no `unwrap`, no `as`
casts, no shell-outs. `tre` is the opposite axis: every public function
touches the filesystem or shells out to git, it has 13 `.unwrap()` calls in
the original source, multiple `bool` parameters, and 8 files that import
each other. It exercises the wrapper's multi-module path (RT-21 fix),
the named-args registry, R0001/R0012/R0014, and the per-file-vs-cross-file
boundary that heck never hit.

Criteria match:
- 1,080 LOC across 8 source files (well within the 500-2000 target)
- Real `std::fs`, `std::process::Command`, walkdir, regex, atty
- 13 `.unwrap()` + 7 `.expect()` in the original
- 5 public functions with `bool` parameters
- No `unsafe`, no proc-macros of its own
- Vendored under MIT — single LICENSE file, single attribution

---

## Methodology

1. Cloned `tre` v0.4.0 (tag `v0.4.0`, commit `2caab28`) into
   `case-studies/tre-strict/` as a standalone crate (not added to workspace
   members; the strict-mode build uses RUSTC_WRAPPER, not the in-tree
   pipeline).
2. Removed `build.rs` (shell-completion generator). It uses
   `include!("src/cli.rs")` to share types with the binary, which conflicts
   with placing `trust_attrs::strict!{}` at the top of `cli.rs` — the
   strict marker is fine for the binary but breaks the
   `include!`-as-uninterpreted-include used by the build script.
3. Added `trust-attrs` as a dependency and `trust_attrs::strict!{}`
   to every `.rs` file under `src/`.
4. Confirmed `cargo build` and `cargo test` worked pre-strict (1 pre-existing
   upstream warning about lifetime elision, no errors).
5. Ran `trust check` on every source file to enumerate violations.
6. Assessed each violation as **real** or **FP** and applied fixes.
7. Re-ran `RUSTC_WRAPPER=… cargo build && cargo test` until clean.
8. Smoke-tested the resulting binary against `src/`.

---

## Pre-conversion metrics

| Metric | Count |
|--------|-------|
| Total source LOC (8 files) | 1,080 |
| `.unwrap()` calls (all) | 13 |
| `.unwrap()` calls outside `#[cfg(test)]` | 4 |
| `.expect(...)` calls | 7 |
| `as` casts | 0 |
| `panic!` / `todo!` / `unreachable!` | 0 |
| Glob imports (`use foo::*`) | 0 |
| Public functions with `bool` param | 5 |
| Module files | 8 |
| `mod` declarations resolved by the wrapper | 7 |

---

## Violation inventory

`trust check` reported the following per-file (initial pass, before
any fixes):

| File | R0042 | R0001 | R0012 | R0014 | Total |
|------|------:|------:|------:|------:|------:|
| `cli.rs` | 0 | 0 | 0 | 0 | 0 |
| `main.rs` | 0 | 0 | 0 | 0 | 0 |
| `tre.rs` | 0 | 0 | 0 | 0 | 0 |
| `diagram_formatting.rs` | 6 | 1 | 1 | 0 | 8 |
| `file_tree.rs` | 3 | 2 | 0 | 4 | 9 |
| `json_formatting.rs` | 3 | 0 | 0 | 2 | 5 |
| `output.rs` | 2 | 0 | 1 | 0 | 3 |
| `path_finders.rs` | 2 | 1 | 3 | 0 | 6 |
| **Total** | **16** | **4** | **5** | **6** | **31** |

(R0001 inside `#[cfg(test)]` blocks is exempt — the 9 `.unwrap()` calls in
test code were not counted.)

---

## Real violations found

### R0012 — bool parameters on public functions (5 instances): REAL

Five public functions take a positional `bool` that is trivially swappable
or misreadable at the call site:

1. `output::print_entries(entries, create_alias: bool, lscolors)` —
   `print_entries(&entries, true, None)` could be a bug or a feature.
2. `diagram_formatting::format_paths(root, children, make_absolute: bool)` —
   the third positional `true`/`false` controls absolute vs relative paths
   and lives next to `Vec<(String, FileType)>`. Swappable with adjacent
   `usize`/`Vec` args at type level only.
3. `path_finders::find_all_paths(root, directories_only: bool, max_depth)` —
   the `bool` sits between two unrelated value types.
4. `path_finders::find_non_hidden_paths(...)` — same shape.
5. `path_finders::find_non_git_ignored_paths(...)` — same shape.

These are textbook R0012 cases. Three of them share the exact same
`(&str, bool, usize)` signature, which is the kind of API where a copy-paste
between modules can flip the bool and produce silent behaviour change.

**Assessment: Real.** Every flagged `bool` is a value the call site
clearly wants to *describe*, not pass anonymously.

**Fix applied:** Introduced two named enums and threaded them through:
- `path_finders::EntryFilter::{All, DirectoriesOnly}`
- `diagram_formatting::PathStyle::{Relative, Absolute}`
- `output::AliasMode::{On, Off}`

Net change: +18 LOC across `path_finders.rs`, `diagram_formatting.rs`,
`output.rs`, `tre.rs` (the call-site translator).

---

### R0001 — `.unwrap()` outside test code (4 instances): REAL

| File:line | Original | Justification for fix |
|-----------|----------|-----------------------|
| `file_tree.rs:143` | `data_option.unwrap()` (after a match over `meta`) | Match returns `None` only on an unresolvable symlink — skip-and-continue is correct. |
| `file_tree.rs:157` | `ancestor.unwrap()` (after `if ancestor.is_none() { continue; }`) | Pattern-match collapse: single `match … { Some(n) => n, None => continue }`. |
| `diagram_formatting.rs:66` | `fs::canonicalize(&file.path).unwrap()` | `canonicalize` fails on broken symlinks and missing files. The fix is to fall back to the relative path instead of panicking the whole `tre` invocation. |
| `path_finders.rs:110` | `.to_str().unwrap()` on a `PathBuf` from git output | `to_str` fails on non-UTF-8 paths. The fix is to `filter_map` the entry out instead of crashing. |

**Assessment: Real.** All four were latent panics that the rule correctly
flagged. The `canonicalize` one in particular would have crashed `tre`
on the very real case of a broken symlink anywhere in the listed tree.

**Fix applied:** Replaced each `unwrap()` with the appropriate
`?`/`match`/fallback (see table above). Net change: +14 LOC.

---

### R0042 — positional args on locally-declared functions (4 instances): REAL

After fixing the false positives (see below), 4 R0042 violations remained
that are clearly real:

| Call site | Callee | Why it's real |
|-----------|--------|---------------|
| `diagram_formatting::format_file(tree, file, history, result, abs)` | local `format_file` arity 5 | Recursive call with 5 same-typed-ish args in a row. The `&FileTree` and `&File` are easily swappable. |
| `diagram_formatting::make_prefix(tree, file, history)` | local arity 3 | `&FileTree` + `&File` look very similar at a glance; swap would compile and produce wrong-looking trees. |
| `path_finders::should_include(entry, root)` | local arity 2 | Both args are `&str`-shaped (well, `&DirEntry` + `&str`, but the inner names are confusable). |
| `output::color_print(text, color)` | local arity 2 | `Display`-bounded `text` + `Option<&ColorSpec>` — type system enforces this one, but the rule still gives a name to the call. |

**Fix applied:** Named args at each call site. Cost: ~12 LOC.

---

## False positives

### FP1: R0042 mis-parses generic types with internal commas (RT-39)

`fn make_prefix(tree: &FileTree, file: &File, format_history: &HashMap<usize, usize>)`
was reported as **arity 4** with the synthetic last param named `usize`:

```
[R0042] call to `make_prefix` must use named arguments (arity 4)
   Help: rewrite as `make_prefix(tree: ..., file: ..., format_history: ..., usize: ...)`
```

The signature has 3 params. The token-stream splitter in
`trust-lower::named_args::split_by_top_comma` is splitting at the
internal comma of `HashMap<usize, usize>` because it doesn't track
generic-argument-list nesting (it tracks closure parens but not angle
brackets).

**Workaround:** Introduced a type alias
`type ChildCount = HashMap<usize, usize>;` and used it in the affected
signatures. This sidesteps the parser bug.

**Followup:** **RT-39** — fix `split_by_top_comma` to track `<…>` depth.

---

### FP2: R0042/R3001 has no cross-file registry (RT-40)

The lower pass builds its function-signature registry per file. This
breaks for any call that crosses a module boundary:

- `FileTree::new(root_path, children)` is declared in `file_tree.rs`.
- It's called from `json_formatting.rs` and `diagram_formatting.rs`.
- The caller's local registry doesn't see the `FileTree::new` signature.
- Worse: if the caller file also has *its own* `new` method (e.g.
  `SerializableTreeNode::new(tree)` in `json_formatting.rs`), the registry
  picks that one up and R0042/R3001 fire on the wrong signature.

The wrapper makes this worse, not better: even with RUSTC_WRAPPER doing
the whole-crate mirror pass, each file is lowered independently
(`trust_lower::lower(&source)` is called per file in
`mirror_module_tree`). There is no whole-crate symbol pass.

**Workaround:** Renamed `SerializableTreeNode::new` → `from_tree` (and
the helper `from` → `build`) to avoid the local-registry collision. For
the `FileTree::new` call we left positional — `new` is not in the local
registry of the caller files after the rename, so R0042 doesn't fire
(the rule falls back to cross-crate behaviour for unknown callees).

**Followup:** **RT-40** — design and implement a whole-crate
callee-signature index so named-args validation works across modules.
This is arguably the largest gap in the dialect for non-toy crates;
heck only escaped it because it was a single-file conversion.

---

### FP3: Method calls collide with free-fn shims of the same short name (RT-41)

`children.insert(name.to_string(), id)` where `children: &mut IndexMap<…>`
fires R0042 because `trust-std` declares a free function
`pub fn insert<K, V>(map, key, value)` for `HashMap`. The local registry
keys callees by short name only, so `xs.insert(…)` and `insert(…)` are
indistinguishable.

The "suggestion" the linter prints is `insert(map: ..., key: ..., value: ...)` —
which makes no sense for a method call where `map` is the receiver.

**Workaround:** Wrote `children.insert(key: …, value: …)` — the named-arg
match drops the un-supplied `map` param and the rule accepts it. This
works but is misleading: a reader assumes the named args correspond to
the method's *actual* parameter list, not a shim's.

**Followup:** **RT-41** — separate the registries (methods vs free
functions) or skip the registry entirely for method-call sites.

---

### FP4: Span fidelity collapses to 1:1 on the strict-marker line (RT-42)

Most R0042 diagnostics report their span as the literal text of line 1
(`trust_attrs::strict!{}`). The R0014 diagnostics also report line
numbers off by 1–6 from the actual position in the source file.

This is the same family as RT-8 (R3001/R0042 span fidelity) but it
re-surfaced on multi-module per-file checking: the lower pass clearly
loses or shifts spans somewhere between parsing and diagnostic emission.

**Followup:** **RT-42** — re-audit span fidelity end-to-end. The user
experience is "everything is wrong on line 1", which makes scanning
errors useless.

**Update (RT-42 fixed):** Root cause was that `trust-lower` did
not enable the `span-locations` feature on `proc-macro2`, so every
diagnostic emitted by the lower pass (R0042 and R3001) used
`byte_range()` that returned `0..0` — which ariadne renders as line 1
col 1, landing on the strict marker. RT-8 had previously fixed the
same class of bug for the syn-AST-based lints (`trust-lints`) by
enabling the feature on that crate; RT-42 mirrors the fix in the
token-stream-based lower pass and threads the offending call paren
group's span and each named arg's name span through
`rewrite_call_args` and `extract_named`. R0042 now points at the
opening paren of the call; R3001 points at the unknown name token.
Regression tests live in `crates/trust-lower/src/named_args.rs`
(`r0042_span_points_at_call_site_not_line_one`,
`r3001_span_points_at_unknown_name_not_line_one`) and in
`crates/trust-lints/src/lib.rs` for R0001/R0005/R0011.

---

### FP5: R0014 on `Slab`/`IndexMap` indexing (RT-43)

`Slab<Box<File>>::index(usize)` is the idiomatic way to access entries
in an arena-style collection. Same for `IndexMap<K, V>::index`. R0014
fires because the index isn't a literal.

These types impl `Index<Key>` and provide `.get(key) -> Option<&V>`
that returns the same data path-panic-free. The fix the rule suggests
(`.get(idx).expect("…")`) works, but it's noisy: each indexing site
grows from `slab[id]` to a 3-line `slab.get(id).expect("invariant …")`
block.

**Assessment:** Mostly real (these *do* panic on bad keys), but the
ergonomic hit is high for arena-style code patterns where the invariant
"key was produced by this slab" is structurally obvious.

**Fix applied:** Used `.get(id).expect(...)` everywhere. Net change:
~12 LOC of expect-string boilerplate.

**Followup:** **RT-43** — consider whether R0014 should permit
`x[id]` when `id` is bound from `x.vacant_entry().key()` or a similar
provenance-tracked pattern, OR whether the rule's documentation should
say "use `.get(id).expect(reason)`" everywhere with no ambition to
auto-detect arena patterns.

---

## Friction points (toolchain-level)

Beyond the FPs above, three things made this conversion painful:

1. **No way to read what the local registry actually contains.** Several
   R0042 errors reported arities and param names I couldn't trace
   without `grep`ing `trust-lower/src/named_args.rs` and
   `trust-std/src/lib.rs`. A `trust explain <file>` that
   dumps the registry would have cut the conversion time roughly in
   half.

2. **`build.rs` shares source with `src/`.** `tre`'s `build.rs` does
   `include!("src/cli.rs")`. The strict marker at the top of `cli.rs`
   is valid Rust (it's a proc-macro call), but `include!`ing it into
   a build script means the build script also has to depend on
   `trust-attrs`, which is brittle. I just dropped `build.rs` and
   the shell-completion output along with it. A real adoption story
   would need either a `#[cfg(not(build_script))]` skip on the marker
   or an explicit "strict mode for build scripts" recipe.

3. **`rustdoc` is still not wrapped (RT-22 / RT-28).** I had to leave
   the `[lib]` section out (the crate is `[[bin]]` only) — if `tre`
   had been a library, the doc-tests would have failed compilation
   the same way heck's did. Real libraries cannot use trust
   end-to-end today.

---

## Changes to vendored source

| Change | LOC | Reason |
|--------|----:|--------|
| Added `trust_attrs::strict!{}` marker to 8 files | +16 | Strict mode activation |
| Added `trust-attrs` to `[dependencies]` | +1 | Strict mode activation |
| Removed `build.rs` and `[build-dependencies]` | −34 | Strict marker conflicts with `include!("src/cli.rs")` |
| Introduced `EntryFilter` enum + threading | +12 | R0012 fix (3 fns) |
| Introduced `PathStyle` enum + threading | +6 | R0012 fix |
| Introduced `AliasMode` enum + threading | +5 | R0012 fix |
| Replaced `.unwrap()` with fallback/match (4 sites) | +14 | R0001 fix |
| Named args on local-fn calls (4 sites) | +12 | R0042 fix |
| `slab[id]` → `slab.get(id).expect(...)` (4 sites) | +12 | R0014 fix |
| Type alias `ChildCount` | +1 | FP1 workaround (RT-39) |
| Renamed `SerializableTreeNode::new` → `from_tree`, `::from` → `::build` | 0 | FP2 workaround (RT-40) |

**Net real bug fixes:** 9 (4 unwrap, 5 bool-param) + 4 R0042 named-args = 13 changes.
**Net FP-driven changes:** Type aliasing, method renaming, and `slab.get().expect()` boilerplate = ~25 LOC across files.

---

## Verdict

For a small CLI of this type (1,080 LOC, real I/O, real error paths,
multi-module), Trust caught:

- **5 real R0012 violations** (`bool` params on public path-finder /
  formatter / output functions). Three of them shared the exact same
  `(&str, bool, usize)` shape — a textbook copy-paste-and-flip footgun.
- **4 real R0001 violations** (`.unwrap()` on `canonicalize`, on a
  unicode-fallible `to_str`, and on options that were "obviously"
  `Some`). The `canonicalize` one in particular was a real latent panic.
- **4 real R0042 violations** (recursive `format_file(tree, file,
  history, result, abs)` and friends, where same-typed adjacent args
  could be silently swapped).

That's **13 real findings in 1,080 LOC** on a battle-tested public CLI.
Two of the four R0001 cases (`fs::canonicalize` and non-UTF-8 paths from
`git ls-files`) are real bugs the upstream maintainers would probably
accept as patches.

The conversion was painful in proportion to **how many modules touch each
other**. heck's 8 files were merged into one because the wrapper didn't
support multi-file; `tre`'s 8 files were kept as-is and exposed the
*next* multi-file limitation — the per-file callee registry (RT-40),
which silently makes named-arg validation incoherent on every
cross-module call. RT-40 is now the most important toolchain followup
for any non-trivial adoption.

For a 500-2,000 LOC CLI with heavy module structure and real I/O,
Trust in its current state can *ship*, but the false-positive
debugging budget is roughly equal to the real-violation-fixing budget.
The signal-to-noise ratio gets meaningfully worse as soon as more than
one file is involved.
