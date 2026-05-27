use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use rustricted_diag::Diagnostic;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Sentinel value (`-`) used in CLI input positions to mean "read from stdin",
/// matching the convention used by `rustc`, `cat`, etc.
const STDIN_SENTINEL: &str = "-";
/// Display label used in diagnostics when the source was read from stdin.
const STDIN_LABEL: &str = "<stdin>";

#[derive(Parser)]
#[command(
    name = "rustricted",
    version,
    about = "A strict Rust dialect for agents",
    long_about = "A strict Rust dialect for agents.\n\n\
                  Pass `-` as the input path to read source from stdin \
                  (e.g. `echo '...' | rustricted check -`). For `build -`, \
                  `--out` must be supplied because there is no input path \
                  to derive the binary name from."
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Parse, lower, lint, and compile a Rustricted source file.
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
    },
    /// Lower a file and print the resulting plain Rust to stdout.
    ///
    /// Pass `-` as the input to read source from stdin.
    Lower {
        /// Input .rs file, or `-` to read from stdin
        input: PathBuf,
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
        Cmd::Check { input } => check(&input),
        Cmd::Lower { input } => lower_to_stdout(&input),
    }
}

fn build(input: &Path, out: Option<&Path>, edition: &str, no_lint: bool) -> Result<()> {
    let (source, label) = read_source(input)?;

    let pipeline = run_pipeline(&label, &source, no_lint)?;

    let tmp = tempfile::Builder::new()
        .prefix("rustricted-")
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

fn check(input: &Path) -> Result<()> {
    let (source, label) = read_source(input)?;
    let _ = run_pipeline(&label, &source, false)?;
    eprintln!("ok: {label}");
    Ok(())
}

fn lower_to_stdout(input: &Path) -> Result<()> {
    let (source, label) = read_source(input)?;
    let pipeline = run_pipeline(&label, &source, true)?;
    print!("{}", pipeline.lowered);
    Ok(())
}

struct PipelineOutput {
    lowered: String,
}

fn run_pipeline(label: &str, source: &str, skip_lints: bool) -> Result<PipelineOutput> {
    let mut all_diagnostics: Vec<Diagnostic> = Vec::new();

    // Lower: rewrite Rustricted extensions to plain Rust.
    let lower_out = rustricted_lower::lower(source).with_context(|| format!("lowering {label}"))?;
    all_diagnostics.extend(lower_out.diagnostics);

    // Parse lowered source for the linters.
    let file: syn::File = syn::parse_str(&lower_out.source)
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
        let lint_report = rustricted_lints::lint_strict(&file, source, lower_out.strict_mode);
        all_diagnostics.extend(lint_report.diagnostics);
    }

    let any_errors = all_diagnostics.iter().any(Diagnostic::is_error);

    if !all_diagnostics.is_empty() {
        let mut stderr = std::io::stderr();
        let _ = rustricted_diag::render(&all_diagnostics, label, source, &mut stderr);
    }

    if any_errors {
        bail!("aborting due to previous errors");
    }

    Ok(PipelineOutput {
        lowered: lower_out.source,
    })
}
