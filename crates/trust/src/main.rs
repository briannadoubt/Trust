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
        Cmd::Check { input, format } => check(&input, format),
        Cmd::Lower { input } => lower_to_stdout(&input),
        Cmd::Index { input, out } => index(&input, out.as_deref()),
        Cmd::Fix { input, write } => fix(&input, write),
        Cmd::Explain { code, format } => explain(code.as_deref(), format),
    }
}

fn build(input: &Path, out: Option<&Path>, edition: &str, no_lint: bool) -> Result<()> {
    let (source, label) = read_source(input)?;

    let pipeline = run_pipeline(&label, &source, no_lint, OutputFormat::Human)?;

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

fn check(input: &Path, format: OutputFormat) -> Result<()> {
    let (source, label) = read_source(input)?;
    let _ = run_pipeline(&label, &source, false, format)?;
    // In JSON mode the document on stdout is the whole result; don't add a
    // human "ok" line that would corrupt it.
    if format == OutputFormat::Human {
        eprintln!("ok: {label}");
    }
    Ok(())
}

fn lower_to_stdout(input: &Path) -> Result<()> {
    let (source, label) = read_source(input)?;
    let pipeline = run_pipeline(&label, &source, true, OutputFormat::Human)?;
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

/// Insert named arguments at positional call sites — vanilla → strict (RT-71).
/// Reads `TRUST_SIGNATURE_PATH` so calls into indexed dependencies are named
/// too. Prints to stdout, or rewrites the file in place with `--write`.
fn fix(input: &Path, write: bool) -> Result<()> {
    let (source, label) = read_source(input)?;
    let extras = trust_lower::sig_index::load_from_env();
    let promoted = trust_lower::promote_named_args(&source, &extras)
        .with_context(|| format!("promoting named arguments in {label}"))?;
    if write {
        if is_stdin(input) {
            bail!("`fix --write` needs a file path to rewrite, not stdin");
        }
        std::fs::write(input, &promoted).with_context(|| format!("writing {}", input.display()))?;
        eprintln!("rewrote {}", input.display());
    } else {
        print!("{promoted}");
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
}

fn run_pipeline(
    label: &str,
    source: &str,
    skip_lints: bool,
    format: OutputFormat,
) -> Result<PipelineOutput> {
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

    let any_errors = all_diagnostics.iter().any(Diagnostic::is_error);

    // RT-70: emit either human-readable (ariadne) or machine-readable JSON.
    // JSON goes to stdout (the document is the whole result); human
    // diagnostics go to stderr so stdout stays clean for `lower`/`build`.
    match format {
        OutputFormat::Human => {
            if !all_diagnostics.is_empty() {
                let mut stderr = std::io::stderr();
                let _ = trust_diag::render(&all_diagnostics, label, source, &mut stderr);
            }
        }
        OutputFormat::Json => {
            print!("{}", trust_diag::to_json(&all_diagnostics, label, source));
        }
    }

    if any_errors {
        bail!("aborting due to previous errors");
    }

    Ok(PipelineOutput {
        lowered: lower_out.source,
    })
}
