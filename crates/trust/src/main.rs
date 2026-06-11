// Stage-0 bootstrap crate (RT-76): plain Rust, built by stock `cargo`.
// The `trust` CLI is part of the toolchain that *implements* the dialect,
// so it cannot be lowered/linted by itself before it exists — it does not
// use the `#![strict]` syntax extensions. The lints are dogfooded on the
// library crates; the named-arg/pipe syntax on the `examples/` fixtures.

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use trust_diag::Diagnostic;

mod config;
use config::TrustConfig;

/// Sentinel value (`-`) used in CLI input positions to mean "read from stdin",
/// matching the convention used by `rustc`, `cat`, etc.
const STDIN_SENTINEL: &str = "-";
/// Display label used in diagnostics when the source was read from stdin.
const STDIN_LABEL: &str = "<stdin>";

#[derive(Parser)]
#[command(
    name = "trust",
    version,
    about = "A strict Rust dialect for agents",
    long_about = "A strict Rust dialect for agents.\n\n\
                  Pass `-` as the input path to read source from stdin \
                  (e.g. `echo '...' | trust check -`). For `build -`, \
                  `--out` must be supplied because there is no input path \
                  to derive the binary name from."
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

/// Diagnostic output format (RT-70).
#[derive(Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
enum OutputFormat {
    /// Human-readable, source-annotated diagnostics (ariadne).
    #[default]
    Human,
    /// Machine-readable JSON for agent consumers: rule, span, line/col,
    /// why, help, and a structured fix with an applicability/confidence.
    Json,
    /// SARIF 2.1.0 for GitHub code-scanning and other static-analysis
    /// consumers (RT-107). `check` only — inline annotations + Security tab.
    Sarif,
}

#[derive(Subcommand)]
enum Cmd {
    /// Parse, lower, lint, and compile a Trust source file.
    ///
    /// Pass `-` as the input to read source from stdin; `--out` is then required.
    Build {
        /// Input .rs file, or `-` to read from stdin
        input: PathBuf,
        /// Output binary path (defaults to input with extension stripped).
        /// Required when reading from stdin.
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Rust edition to pass to rustc
        #[arg(long, default_value = "2021")]
        edition: String,
        /// Skip lints (useful when bootstrapping non-strict files).
        #[arg(long)]
        no_lint: bool,
    },
    /// Parse and lint a file without compiling.
    ///
    /// Pass `-` as the input to read source from stdin.
    Check {
        /// Input .rs file, or `-` to read from stdin
        input: PathBuf,
        /// Diagnostic output format (RT-70)
        #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
        format: OutputFormat,
        /// Which rules to run (RT-101).
        ///
        /// `all` (default) runs the full strict rule set through the normal
        /// lower+lint pipeline — it expects the dialect (named-arg syntax,
        /// `#![strict]`/metadata activation). `bugs` runs only the
        /// runtime-bug-catching lints (`.unwrap()`, `as`-casts, bare indexing,
        /// dropped error context, …); `safety` runs every rule that applies to
        /// *plain* Rust (the bug rules plus the style/design ones), minus the
        /// named-arg dialect (R0042). Both advisory modes need no `#![strict]`
        /// marker and keep your source valid stock Rust — so Trust works as an
        /// out-of-tree linter on a stock cargo workspace. You may also pass an
        /// explicit comma list of rule codes, e.g. `--rules R0001,R0003,R0014`.
        /// When omitted, the `rules` key in a discovered `trust.toml` (RT-102)
        /// is used, falling back to `all`.
        #[arg(long)]
        rules: Option<String>,
    },
    /// Lower a file and print the resulting plain Rust to stdout.
    ///
    /// Pass `-` as the input to read source from stdin.
    Lower {
        /// Input .rs file, or `-` to read from stdin
        input: PathBuf,
    },
    /// Extract a crate's public-fn signature index to a manifest (RT-66).
    ///
    /// The manifest lets a *dependent* crate enforce named arguments
    /// (R0042) on calls into this one: generate it here, then point the
    /// dependent build at it via `TRUST_SIGNATURE_PATH=<file-or-dir>`
    /// alongside the `trust-rustc` wrapper. Input may be a crate `src/`
    /// directory, a single `.rs` file, or `-` for stdin.
    Index {
        /// Crate `src/` directory, a single `.rs` file, or `-` for stdin
        input: PathBuf,
        /// Manifest output path (defaults to stdout)
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Insert named arguments at positional call sites (vanilla → strict).
    ///
    /// Splices `name:` before each positional argument of a call to a
    /// function Trust can see — in-crate `fn`s plus any dependency indices
    /// from `TRUST_SIGNATURE_PATH` (RT-71). Only the names are inserted;
    /// all other formatting is preserved. Pass `-` to read from stdin.
    Fix {
        /// Input .rs file, or `-` to read from stdin
        input: PathBuf,
        /// Rewrite the file in place instead of printing to stdout
        #[arg(short, long)]
        write: bool,
        /// Apply mechanical safety fixes instead of named-arg insertion
        /// (RT-106): rewrite `.unwrap()`/`.expect(…)` to `?` inside functions
        /// that return `Result`. Produces plain Rust (no dialect), so it's the
        /// companion to `check --rules bugs`. Best-effort — review the result.
        #[arg(long)]
        safety: bool,
    },
    /// Scaffold a new strict Trust project (RT-94).
    ///
    /// Creates `<name>/` with a `Cargo.toml` that opts into strict mode via
    /// `[package.metadata.trust] strict = true`, a hello `src/main.rs` that
    /// exercises named-argument syntax, a `.gitignore`, and a `README.md`.
    /// Build it with `cargo trustc build` (not plain cargo).
    New {
        /// Name of the project (and the directory to create)
        name: String,
    },
    /// Explain Trust's rules — the proactive agent contract (RT-78).
    ///
    /// With a rule code, explains that rule; with no code, lists the whole
    /// catalogue. Each entry gives the rationale (why) and the canonical
    /// compliant idiom (what to write instead). `--format json` emits a
    /// machine-readable catalogue an agent harness can load into context.
    Explain {
        /// A rule code like `R0017` (omit to list every rule)
        code: Option<String>,
        /// Output format
        #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
        format: OutputFormat,
    },
}

/// Returns true when the CLI input path is the stdin sentinel `-`.
fn is_stdin(input: &Path) -> bool {
    input.as_os_str() == STDIN_SENTINEL
}

/// Read source from either a file or stdin (when `input` is `-`). The returned
/// label is used in diagnostic output.
fn read_source(input: &Path) -> Result<(String, String)> {
    if is_stdin(input) {
        let mut buf = String::new();
        io::stdin()
            .lock()
            .read_to_string(&mut buf)
            .context("reading source from stdin")?;
        Ok((buf, STDIN_LABEL.to_string()))
    } else {
        let source = std::fs::read_to_string(input)
            .with_context(|| format!("reading {}", input.display()))?;
        Ok((source, input.display().to_string()))
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Cmd::Build {
            input,
            out,
            edition,
            no_lint,
        } => build(&input, out.as_deref(), &edition, no_lint),
        Cmd::Check {
            input,
            format,
            rules,
        } => check(&input, format, rules.as_deref()),
        Cmd::Lower { input } => lower_to_stdout(&input),
        Cmd::Index { input, out } => index(&input, out.as_deref()),
        Cmd::Fix {
            input,
            write,
            safety,
        } => fix(&input, write, safety),
        Cmd::New { name } => scaffold_new(&name),
        Cmd::Explain { code, format } => explain(code.as_deref(), format),
    }
}

fn build(input: &Path, out: Option<&Path>, edition: &str, no_lint: bool) -> Result<()> {
    let (source, label) = read_source(input)?;

    let pipeline = compute_pipeline(&label, &source, no_lint)?;
    emit_diagnostics(&pipeline.diagnostics, &label, &source, OutputFormat::Human);
    if pipeline.diagnostics.iter().any(Diagnostic::is_error) {
        bail!("aborting due to previous errors");
    }

    let tmp = tempfile::Builder::new()
        .prefix("trust-")
        .suffix(".rs")
        .tempfile()
        .context("creating temporary lowered source file")?;
    std::fs::write(tmp.path(), &pipeline.lowered)
        .with_context(|| format!("writing lowered source to {}", tmp.path().display()))?;

    // When reading from stdin there is no input filename to derive `-o` from,
    // so `--out` becomes mandatory. Surface that as a clear error.
    let default_out;
    let out_path: &Path = if let Some(p) = out {
        p
    } else if is_stdin(input) {
        bail!("`build -` reads source from stdin; pass `--out PATH` to specify the binary path");
    } else {
        default_out = input.with_extension("");
        &default_out
    };

    let status = Command::new("rustc")
        .arg(tmp.path())
        .arg("-o")
        .arg(out_path)
        .arg(format!("--edition={edition}"))
        .status()
        .context("invoking rustc")?;

    if !status.success() {
        bail!("rustc failed with status {status}");
    }

    eprintln!("built {}", out_path.display());
    Ok(())
}

/// How `trust check` selects which rules to run (RT-101).
enum RuleSelection {
    /// The full strict set via the normal lower+lint pipeline. Expects the
    /// dialect (named args, `#![strict]`/metadata activation).
    All,
    /// A dialect-free subset run as an advisory pass over plain Rust — no
    /// `#![strict]` marker, no lowering, no R0042.
    Advisory(Vec<trust_lints::Rule>),
}

/// Parse the `--rules` value: `all`, the `safety`/`bugs` group, or an explicit
/// comma list of rule codes. Dialect rules (R0042) are rejected in advisory
/// selections with a message pointing at the full pipeline.
fn parse_rule_selection(spec: &str) -> Result<RuleSelection> {
    use trust_lints::Rule;
    match spec.trim().to_ascii_lowercase().as_str() {
        "all" => Ok(RuleSelection::All),
        "bugs" => Ok(RuleSelection::Advisory(trust_lints::bug_rules())),
        "safety" => Ok(RuleSelection::Advisory(trust_lints::advisory_rules())),
        _ => {
            let mut rules = Vec::new();
            for code in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                let rule = Rule::from_code(&code.to_uppercase()).with_context(|| {
                    format!("unknown rule code `{code}` in --rules (run `trust explain` for the catalogue)")
                })?;
                if rule.is_dialect() {
                    bail!(
                        "rule {code} ({}) needs Trust's named-argument syntax, which stock rustc \
                         rejects — it can't run as an advisory lint. Use `--rules all` with \
                         `cargo trustc` for the full dialect.",
                        rule.name()
                    );
                }
                rules.push(rule);
            }
            if rules.is_empty() {
                bail!("--rules got an empty selection; pass `all`, `safety`, or a comma list of rule codes");
            }
            Ok(RuleSelection::Advisory(rules))
        }
    }
}

fn check(input: &Path, format: OutputFormat, rules_flag: Option<&str>) -> Result<()> {
    // RT-102: a discovered `trust.toml` supplies the default rule selection and
    // the project-wide allow/warn lists. The `--rules` flag overrides `rules`.
    let config = TrustConfig::discover(input)?;
    let spec = rules_flag
        .map(str::to_string)
        .or_else(|| config.rules.clone())
        .unwrap_or_else(|| "all".to_string());
    let selection = parse_rule_selection(&spec)?;

    // stdin, or a single (non-manifest) file: check exactly that input.
    if is_stdin(input) || (input.is_file() && !is_manifest(input)) {
        return check_one(input, format, &selection, &config);
    }

    // A directory or a Cargo.toml: walk the project tree and check every `.rs`
    // file under it in one command (RT-105). For a manifest the tree is the
    // directory that contains it — which, for a workspace root, covers all
    // member crates, since their sources live beneath it. No more injecting
    // markers into a temp copy of every file in a shell loop.
    let root = if is_manifest(input) && input.is_file() {
        match input.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from("."),
        }
    } else if input.is_dir() {
        input.to_path_buf()
    } else {
        // Non-existent / unrecognized path: defer to read_source for a precise
        // "reading <path>" error rather than a vague "no files found".
        return check_one(input, format, &selection, &config);
    };

    let files = collect_rs_files(&root);
    if files.is_empty() {
        bail!("no .rs files found under {}", root.display());
    }
    check_many(&files, format, &selection, &config)
}

/// `true` if the path names a cargo manifest (`Cargo.toml`).
fn is_manifest(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n == "Cargo.toml")
}

/// Compute diagnostics for one source string under the given rule selection.
/// `All` runs the full lower+lint pipeline; an advisory selection parses plain
/// Rust and runs the chosen subset with no marker and no lowering (RT-101).
fn compute_diagnostics(
    label: &str,
    source: &str,
    selection: &RuleSelection,
    config: &TrustConfig,
) -> Result<Vec<Diagnostic>> {
    let mut diagnostics = match selection {
        RuleSelection::All => compute_pipeline(label, source, false)?.diagnostics,
        RuleSelection::Advisory(rules) => {
            let file: syn::File =
                syn::parse_str(source).with_context(|| format!("parsing {label} as Rust"))?;
            trust_lints::lint_advisory(&file, source, rules.clone()).diagnostics
        }
    };
    // RT-102: drop project-allowed codes; downgrade project-relaxed ones.
    config.apply(&mut diagnostics);
    Ok(diagnostics)
}

/// Check a single input (a `.rs` file or stdin) and render its diagnostics.
fn check_one(
    input: &Path,
    format: OutputFormat,
    selection: &RuleSelection,
    config: &TrustConfig,
) -> Result<()> {
    let (source, label) = read_source(input)?;
    let diagnostics = compute_diagnostics(&label, &source, selection, config)?;
    emit_diagnostics(&diagnostics, &label, &source, format);
    if format == OutputFormat::Human && diagnostics.is_empty() {
        eprintln!("ok: {label}");
    }
    if diagnostics.iter().any(Diagnostic::is_error) {
        bail!("aborting due to previous errors");
    }
    Ok(())
}

/// Check every file in a walked tree (RT-105), aggregating results. Resilient
/// to a single bad file: a read/parse/lower failure is reported and counted but
/// does not abort the run, so one unparseable file can't mask findings in the
/// rest (the mirror's "abort at first failing entry" trap, per past sessions).
/// Exits non-zero if any file has an error-level finding or failed to process.
fn check_many(
    files: &[PathBuf],
    format: OutputFormat,
    selection: &RuleSelection,
    config: &TrustConfig,
) -> Result<()> {
    let mut findings = 0usize;
    let mut files_with_findings = 0usize;
    let mut failed = 0usize;
    let mut errors = 0usize;
    // JSON mode accumulates one document per file and emits a single array.
    let mut json_docs: Vec<String> = Vec::new();
    // SARIF mode accumulates (uri, source, diagnostics) for one whole-run log.
    let mut sarif_files: Vec<(String, String, Vec<Diagnostic>)> = Vec::new();

    for path in files {
        let label = path.display().to_string();
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("trust: skipping {label}: {e}");
                failed += 1;
                continue;
            }
        };
        match compute_diagnostics(&label, &source, selection, config) {
            Ok(diags) => {
                if !diags.is_empty() {
                    files_with_findings += 1;
                    findings += diags.len();
                    errors += diags.iter().filter(|d| d.is_error()).count();
                }
                match format {
                    OutputFormat::Human => {
                        emit_diagnostics(&diags, &label, &source, OutputFormat::Human)
                    }
                    OutputFormat::Json => json_docs.push(trust_diag::to_json(
                        &diags,
                        trust_diag::NamedSource {
                            name: &label,
                            text: &source,
                        },
                    )),
                    OutputFormat::Sarif => sarif_files.push((label, source, diags)),
                }
            }
            Err(e) => {
                eprintln!("trust: skipping {label}: {e:#}");
                failed += 1;
            }
        }
    }

    if format == OutputFormat::Json {
        // A JSON array of per-file result objects. Single-file `check` still
        // emits one bare object, unchanged (RT-70).
        print!("[{}]", json_docs.join(","));
    } else if format == OutputFormat::Sarif {
        // One SARIF run covering every file walked (RT-107).
        let fds: Vec<trust_diag::FileDiagnostics> = sarif_files
            .iter()
            .map(|(name, text, diagnostics)| trust_diag::FileDiagnostics {
                name,
                text,
                diagnostics,
            })
            .collect();
        print!("{}", trust_diag::to_sarif(&fds));
    } else {
        let scanned = files.len();
        if findings == 0 && failed == 0 {
            eprintln!("ok: {scanned} files clean");
        } else {
            let skipped = if failed > 0 {
                format!("; {failed} skipped (unreadable/unparseable)")
            } else {
                String::new()
            };
            let warns = findings - errors;
            let warn_note = if warns > 0 {
                format!(" ({errors} error(s), {warns} warning(s))")
            } else {
                String::new()
            };
            eprintln!(
                "{findings} finding(s){warn_note} in {files_with_findings} of {scanned} file(s){skipped}"
            );
        }
    }

    // Warnings (project-relaxed rules) don't fail the run; errors and
    // unprocessable files do.
    if errors > 0 || failed > 0 {
        bail!("aborting due to previous errors");
    }
    Ok(())
}

/// Recursively collect `.rs` files under `root`, skipping build output and VCS
/// metadata (`target/`, any dotfile/dir like `.git`). Returns a sorted list so
/// output is deterministic across runs and platforms. A subdirectory that
/// can't be read is skipped rather than aborting the whole walk.
fn collect_rs_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                // Skip build output and hidden dirs when descending. The root
                // itself is always walked even if the user named a dotdir.
                if name == "target" || name.starts_with('.') {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn lower_to_stdout(input: &Path) -> Result<()> {
    let (source, label) = read_source(input)?;
    let pipeline = compute_pipeline(&label, &source, true)?;
    print!("{}", pipeline.lowered);
    Ok(())
}

/// Extract the public-fn signature index of `input` (a crate `src/` dir, a
/// single `.rs` file, or stdin) and emit it as a `name:p1,p2` manifest —
/// either to `out` or to stdout. See `trust_lower::sig_index` (RT-66).
fn index(input: &Path, out: Option<&Path>) -> Result<()> {
    let entries = if is_stdin(input) {
        let (source, _label) = read_source(input)?;
        trust_lower::sig_index::extract_from_source(&source)
    } else if input.is_dir() {
        trust_lower::sig_index::extract_from_dir(input)
    } else {
        let (source, _label) = read_source(input)?;
        trust_lower::sig_index::extract_from_source(&source)
    };

    let header = format!(
        "# @generated by `trust index {}` — Trust public-fn signature index.\n\
         # Point a dependent build at this file via TRUST_SIGNATURE_PATH so\n\
         # R0042 / named-arg reordering apply to calls into this crate.",
        input.display()
    );
    let manifest = trust_lower::sig_index::render_manifest(&entries, &header);

    match out {
        Some(path) => {
            std::fs::write(path, &manifest)
                .with_context(|| format!("writing signature manifest to {}", path.display()))?;
            eprintln!("wrote {} signatures to {}", entries.len(), path.display());
        }
        None => print!("{manifest}"),
    }
    Ok(())
}

/// Rewrite a source file: by default insert named arguments at positional call
/// sites (vanilla → strict, RT-71, reading `TRUST_SIGNATURE_PATH` for indexed
/// dependencies); with `--safety` (RT-106), rewrite `.unwrap()`/`.expect(…)` to
/// `?` inside `Result`-returning functions instead. Prints to stdout, or
/// rewrites the file in place with `--write`.
fn fix(input: &Path, write: bool, safety: bool) -> Result<()> {
    let (source, label) = read_source(input)?;
    let rewritten = if safety {
        trust_lower::fix_unwrap_to_question(&source)
            .with_context(|| format!("applying safety fixes in {label}"))?
    } else {
        let extras = trust_lower::sig_index::load_from_env();
        trust_lower::promote_named_args(&source, &extras)
            .with_context(|| format!("promoting named arguments in {label}"))?
    };
    if write {
        if is_stdin(input) {
            bail!("`fix --write` needs a file path to rewrite, not stdin");
        }
        std::fs::write(input, &rewritten)
            .with_context(|| format!("writing {}", input.display()))?;
        eprintln!("rewrote {}", input.display());
    } else {
        print!("{rewritten}");
    }
    Ok(())
}

/// The drop-in agent-instructions block (RT-95) written to a scaffolded
/// project's `CLAUDE.md`. Kept in the binary (installed copies don't have the
/// repo); a unit test asserts docs/templates/CLAUDE-md-snippet.md contains it
/// verbatim so the two can't drift.
const CLAUDE_MD_BLOCK: &str = r#"## Trust (strict Rust dialect)

This project uses Trust, a strict Rust dialect that lowers to plain Rust.
Build, run, and test with `cargo trustc build|run|test` — NEVER plain `cargo`:
named-argument syntax won't parse under stock cargo, and that's expected.

- Calls with more than one argument use named arguments:
  `make_rect(width: 1920, height: 1080)`. Also available: the pipe operator
  `e |> f(args)` and `requires!(cond)` preconditions.
- Build errors with R-codes (R0001, R0042, …) are Trust teaching errors:
  read the `why:` and `help:`/`instead:` text, apply it, then rebuild.
  - `trust explain <CODE>` — detail on one rule.
  - `trust fix <file> --write` — auto-inserts argument names.
  - `cargo trustc build --message-format json` — machine-readable diagnostics.
- `#[cfg(test)]` code is exempt — write tests in plain Rust; don't convert them.
- Don't suppress rules without a `reason`. Suppression via
  `#[allow(trust::R0xxx, reason = "…")]` only compiles under cargo trustc,
  never stock cargo.

Docs: https://github.com/briannadoubt/Trust — see `docs/WRITING-TRUST.md`
for the full agent guide.
"#;

/// Scaffold a new strict Trust project (RT-94): a standalone cargo package
/// with `[package.metadata.trust] strict = true` and a hello `main.rs` that
/// uses named-argument syntax, proving the toolchain works on first run.
fn scaffold_new(name: &str) -> Result<()> {
    validate_project_name(name)?;

    let root = Path::new(name);
    if root.exists() {
        bail!(
            "destination `{}` already exists — pick another name or remove it first",
            root.display()
        );
    }

    let cargo_toml = format!(
        "[package]\n\
         name = \"{name}\"\n\
         version = \"0.1.0\"\n\
         edition = \"2021\"\n\
         \n\
         # Strict mode is enforced by `cargo trustc` (build/run/test); stock cargo\n\
         # ignores this metadata table entirely.\n\
         [package.metadata.trust]\n\
         strict = true\n"
    );

    let main_rs = "\
// Named-argument call syntax below only compiles via `cargo trustc build` —
// plain `cargo build` will reject it.

fn make_point(x: i32, y: i32) -> (i32, i32) {
    (x, y)
}

fn main() {
    let point = make_point(x: 1, y: 2);
    println!(\"point = {point:?}\");
}
";

    let readme = format!(
        "# {name}\n\
         \n\
         A [Trust](https://github.com/briannadoubt/Trust) project — a strict Rust\n\
         dialect that lowers to plain Rust at build time.\n\
         \n\
         Build and run with `cargo trustc` (NOT plain cargo):\n\
         \n\
         ```sh\n\
         cargo trustc build\n\
         cargo trustc run\n\
         ```\n"
    );

    let src = root.join("src");
    std::fs::create_dir_all(&src)
        .with_context(|| format!("creating directory {}", src.display()))?;

    let write = |path: PathBuf, contents: &str| -> Result<()> {
        std::fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))
    };
    write(root.join("Cargo.toml"), &cargo_toml)?;
    write(src.join("main.rs"), main_rs)?;
    write(root.join(".gitignore"), "/target\n")?;
    write(root.join("README.md"), &readme)?;
    write(root.join("CLAUDE.md"), CLAUDE_MD_BLOCK)?;

    eprintln!("created strict project `{name}` — try `cd {name} && cargo trustc run`");
    Ok(())
}

/// A plausible crate name: non-empty, only `[a-zA-Z0-9_-]`, and not starting
/// with a digit.
fn validate_project_name(name: &str) -> Result<()> {
    let valid_char = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    match name.chars().next() {
        None => bail!("project name must not be empty"),
        Some(c) if c.is_ascii_digit() => {
            bail!("invalid project name `{name}`: must not start with a digit")
        }
        _ => {}
    }
    if let Some(bad) = name.chars().find(|&c| !valid_char(c)) {
        bail!(
            "invalid project name `{name}`: character `{bad}` is not allowed \
             (use letters, digits, `_`, or `-`)"
        );
    }
    Ok(())
}

/// Explain Trust's rules (RT-78) — the proactive agent contract. With a code,
/// explains one rule; otherwise lists the whole catalogue. `--format json`
/// yields a machine-readable catalogue an agent can load into context.
fn explain(code: Option<&str>, format: OutputFormat) -> Result<()> {
    use trust_lints::Rule;
    let rules: Vec<Rule> = match code {
        Some(c) => match Rule::from_code(&c.to_uppercase()) {
            Some(r) => vec![r],
            None => bail!("unknown rule code `{c}` — run `trust explain` to list every rule"),
        },
        None => trust_lints::all_rules(),
    };
    match format {
        OutputFormat::Json => print!("{}", explain_json(&rules)),
        OutputFormat::Human => print!("{}", explain_human(&rules)),
        OutputFormat::Sarif => {
            bail!("`--format sarif` describes findings, not rules — it's only supported by `trust check`")
        }
    }
    Ok(())
}

fn explain_human(rules: &[trust_lints::Rule]) -> String {
    let mut out = String::new();
    for r in rules {
        out.push_str(&format!("{}  {}  (error)\n", r.code(), r.name()));
        out.push_str(&format!("  why:     {}\n", r.rationale()));
        out.push_str(&format!("  instead: {}\n\n", r.instead()));
    }
    out
}

fn explain_json(rules: &[trust_lints::Rule]) -> String {
    let mut out = String::from("{\n  \"version\": \"0.1\",\n  \"rules\": [");
    for (i, r) in rules.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("\n    {\"code\": ");
        out.push_str(&trust_diag::json_escape(r.code()));
        out.push_str(", \"name\": ");
        out.push_str(&trust_diag::json_escape(r.name()));
        out.push_str(", \"severity\": \"error\", \"why\": ");
        out.push_str(&trust_diag::json_escape(r.rationale()));
        out.push_str(", \"instead\": ");
        out.push_str(&trust_diag::json_escape(r.instead()));
        out.push('}');
    }
    if rules.is_empty() {
        out.push_str("]\n}\n");
    } else {
        out.push_str("\n  ]\n}\n");
    }
    out
}

struct PipelineOutput {
    lowered: String,
    diagnostics: Vec<Diagnostic>,
}

/// Lower + lint one source string and return the lowered Rust alongside the
/// collected diagnostics. Pure computation: it does not render or exit — the
/// caller decides how to present results and whether errors are fatal, so the
/// same pipeline drives single-file `check`/`build`, `lower`, and the
/// multi-file workspace walk (RT-105).
fn compute_pipeline(label: &str, source: &str, skip_lints: bool) -> Result<PipelineOutput> {
    let mut all_diagnostics: Vec<Diagnostic> = Vec::new();

    // Lower: rewrite Trust extensions to plain Rust. RT-66: seed the
    // callee registry with any dependency signature indices named by
    // `TRUST_SIGNATURE_PATH`, so cross-crate calls get the same R0042 /
    // named-arg reordering treatment as in-crate ones.
    let dep_extras = trust_lower::sig_index::load_from_env();
    let lower_out = trust_lower::lower_with_extra_callees(source, &dep_extras)
        .with_context(|| format!("lowering {label}"))?;
    all_diagnostics.extend(lower_out.diagnostics);

    // Parse lowered source for the linters.
    // lint_source, not source: the linter's allow map is built from the
    // `#[allow(trust::…)]` attributes, which are stripped from the
    // rustc-facing `source` (RT-89).
    let file: syn::File = syn::parse_str(&lower_out.lint_source)
        .with_context(|| format!("re-parsing lowered source from {label}"))?;

    // Lints (only fire in `#![strict]` files; safe to skip on bootstrap).
    // strict_mode comes from the lowering pass — it reads `#![strict]` from
    // the original token stream before that attribute is stripped for rustc.
    //
    // NOTE: pass the *original* source string (not lower_out.source) so that
    // comment-window checks (R0005 "// safety:", R0006 "// reason:") can find
    // their justification comments. prettyplease strips all comments from the
    // lowered output, which would make those rules fire unconditionally if we
    // passed lower_out.source here.
    if !skip_lints {
        let lint_report = trust_lints::lint_strict(&file, source, lower_out.strict_mode);
        all_diagnostics.extend(lint_report.diagnostics);
    }

    Ok(PipelineOutput {
        lowered: lower_out.source,
        diagnostics: all_diagnostics,
    })
}

/// Render diagnostics in the requested format (RT-70). JSON goes to stdout (the
/// document is the whole result); human-readable (ariadne) diagnostics go to
/// stderr so stdout stays clean for `lower`/`build`. Shared by the full
/// pipeline and the advisory pass (RT-101).
fn emit_diagnostics(diags: &[Diagnostic], label: &str, source: &str, format: OutputFormat) {
    match format {
        OutputFormat::Human => {
            if !diags.is_empty() {
                let mut stderr = std::io::stderr();
                let _ = trust_diag::render(
                    diags,
                    trust_diag::NamedSource {
                        name: label,
                        text: source,
                    },
                    &mut stderr,
                );
            }
        }
        OutputFormat::Json => {
            print!(
                "{}",
                trust_diag::to_json(
                    diags,
                    trust_diag::NamedSource {
                        name: label,
                        text: source,
                    },
                )
            );
        }
        OutputFormat::Sarif => {
            print!(
                "{}",
                trust_diag::to_sarif(&[trust_diag::FileDiagnostics {
                    name: label,
                    text: source,
                    diagnostics: diags,
                }])
            );
        }
    }
}

#[cfg(test)]
mod tests {
    /// RT-95 drift guard: the agent-instructions block baked into the binary
    /// must appear verbatim in docs/templates/CLAUDE-md-snippet.md.
    #[test]
    fn claude_md_block_matches_docs_template() {
        let docs_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/templates/CLAUDE-md-snippet.md"
        );
        let docs = std::fs::read_to_string(docs_path)
            .unwrap_or_else(|e| panic!("reading {docs_path}: {e}"));
        assert!(
            docs.contains(super::CLAUDE_MD_BLOCK),
            "docs/templates/CLAUDE-md-snippet.md no longer contains the \
             CLAUDE_MD_BLOCK const verbatim — update one to match the other"
        );
    }

    // RT-105: the workspace walk must ignore build output and VCS metadata so
    // a single `trust check <dir>` doesn't drown in `target/` artifacts.
    #[test]
    fn collect_rs_files_skips_target_and_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("target/debug")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn a() {}").unwrap();
        std::fs::write(root.join("build.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("target/debug/gen.rs"), "fn g() {}").unwrap();
        std::fs::write(root.join(".git/hook.rs"), "fn h() {}").unwrap();
        std::fs::write(root.join("src/notes.txt"), "nope").unwrap();

        let names: Vec<String> = super::collect_rs_files(root)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"lib.rs".to_string()));
        assert!(names.contains(&"build.rs".to_string()));
        assert!(!names.iter().any(|n| n == "gen.rs"), "target/ must be skipped");
        assert!(!names.iter().any(|n| n == "hook.rs"), "hidden dirs skipped");
        assert!(!names.iter().any(|n| n == "notes.txt"), "non-.rs skipped");
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn is_manifest_matches_cargo_toml_exactly() {
        use std::path::Path;
        assert!(super::is_manifest(Path::new("Cargo.toml")));
        assert!(super::is_manifest(Path::new("/a/b/Cargo.toml")));
        assert!(!super::is_manifest(Path::new("src/lib.rs")));
        assert!(!super::is_manifest(Path::new("cargo.toml")));
    }
}
