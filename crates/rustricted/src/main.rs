use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use rustricted_diag::Diagnostic;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Parser)]
#[command(
    name = "rustricted",
    version,
    about = "A strict Rust dialect for agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Parse, lower, lint, and compile a Rustricted source file.
    Build {
        /// Input .rs file
        input: PathBuf,
        /// Output binary path (defaults to input with extension stripped)
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
    Check {
        /// Input .rs file
        input: PathBuf,
    },
    /// Lower a file and print the resulting plain Rust to stdout.
    Lower {
        /// Input .rs file
        input: PathBuf,
    },
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
    let source =
        std::fs::read_to_string(input).with_context(|| format!("reading {}", input.display()))?;

    let pipeline = run_pipeline(input, &source, no_lint)?;

    let tmp = tempfile::Builder::new()
        .prefix("rustricted-")
        .suffix(".rs")
        .tempfile()
        .context("creating temporary lowered source file")?;
    std::fs::write(tmp.path(), &pipeline.lowered)
        .with_context(|| format!("writing lowered source to {}", tmp.path().display()))?;

    let default_out = input.with_extension("");
    let out_path = out.unwrap_or(&default_out);

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
    let source =
        std::fs::read_to_string(input).with_context(|| format!("reading {}", input.display()))?;
    let _ = run_pipeline(input, &source, false)?;
    eprintln!("ok: {}", input.display());
    Ok(())
}

fn lower_to_stdout(input: &Path) -> Result<()> {
    let source =
        std::fs::read_to_string(input).with_context(|| format!("reading {}", input.display()))?;
    let pipeline = run_pipeline(input, &source, true)?;
    print!("{}", pipeline.lowered);
    Ok(())
}

struct PipelineOutput {
    lowered: String,
}

fn run_pipeline(input: &Path, source: &str, skip_lints: bool) -> Result<PipelineOutput> {
    let filename = input.display().to_string();
    let mut all_diagnostics: Vec<Diagnostic> = Vec::new();

    // Lower: rewrite Rustricted extensions to plain Rust.
    let lower_out =
        rustricted_lower::lower(source).with_context(|| format!("lowering {}", input.display()))?;
    all_diagnostics.extend(lower_out.diagnostics);

    // Parse lowered source for the linters.
    let file: syn::File = syn::parse_str(&lower_out.source)
        .with_context(|| format!("re-parsing lowered source from {}", input.display()))?;

    // Lints (only fire in `#![strict]` files; safe to skip on bootstrap).
    // strict_mode comes from the lowering pass — it reads `#![strict]` from
    // the original token stream before that attribute is stripped for rustc.
    if !skip_lints {
        let lint_report =
            rustricted_lints::lint_strict(&file, &lower_out.source, lower_out.strict_mode);
        all_diagnostics.extend(lint_report.diagnostics);

    }

    let any_errors = all_diagnostics.iter().any(Diagnostic::is_error);

    if !all_diagnostics.is_empty() {
        let mut stderr = std::io::stderr();
        let _ = rustricted_diag::render(&all_diagnostics, &filename, source, &mut stderr);
    }

    if any_errors {
        bail!("aborting due to previous errors");
    }

    Ok(PipelineOutput {
        lowered: lower_out.source,
    })
}
