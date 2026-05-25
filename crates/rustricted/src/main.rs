use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
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
    /// Parse, lower, and compile a Rustricted source file.
    Build {
        /// Input .rs file
        input: PathBuf,
        /// Output binary path (defaults to input with extension stripped)
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Rust edition to pass to rustc
        #[arg(long, default_value = "2021")]
        edition: String,
    },
    /// Parse and lint a file without compiling.
    Check {
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
        } => build(&input, out.as_deref(), &edition),
        Cmd::Check { input } => check(&input),
    }
}

fn build(input: &Path, out: Option<&Path>, edition: &str) -> Result<()> {
    let source =
        std::fs::read_to_string(input).with_context(|| format!("reading {}", input.display()))?;

    let lowered = rustricted_syntax::roundtrip(&source)
        .with_context(|| format!("parsing {}", input.display()))?;

    let tmp = tempfile::Builder::new()
        .prefix("rustricted-")
        .suffix(".rs")
        .tempfile()
        .context("creating temporary lowered source file")?;
    std::fs::write(tmp.path(), &lowered)
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
    rustricted_syntax::roundtrip(&source)
        .with_context(|| format!("parsing {}", input.display()))?;
    eprintln!("ok: {}", input.display());
    Ok(())
}
