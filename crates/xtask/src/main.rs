//! Developer tooling. Currently:
//!
//! - `cargo xtask gen-docs` — regenerates auto-marked sections in `docs/SPEC.md`
//!   from the `rustricted-lints` `Rule` catalogue.
//! - `cargo xtask gen-docs --check` — exit non-zero if docs would change (CI).

use anyhow::{bail, Context, Result};
use std::env;
use std::fs;
use std::path::PathBuf;

const LINTS_BEGIN: &str = "<!-- BEGIN auto-generated: lints-table -->";
const LINTS_END: &str = "<!-- END auto-generated: lints-table -->";

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let cmd = args.next().context("usage: cargo xtask <cmd> [--check]")?;
    let rest: Vec<String> = args.collect();
    let check_only = rest.iter().any(|a| a == "--check");

    match cmd.as_str() {
        "gen-docs" => gen_docs(check_only),
        other => bail!("unknown subcommand: {other}"),
    }
}

fn repo_root() -> Result<PathBuf> {
    // CARGO_MANIFEST_DIR is crates/xtask; parent twice is the workspace root.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(|p| p.parent())
        .context("could not derive workspace root from CARGO_MANIFEST_DIR")?
        .to_path_buf();
    Ok(root)
}

fn gen_docs(check_only: bool) -> Result<()> {
    let root = repo_root()?;
    let spec_path = root.join("docs/SPEC.md");
    let original = fs::read_to_string(&spec_path)
        .with_context(|| format!("reading {}", spec_path.display()))?;

    let table = build_lints_table();
    let updated = replace_section(&original, LINTS_BEGIN, LINTS_END, &table)?;

    if check_only {
        if updated != original {
            eprintln!("docs/SPEC.md is stale — run `cargo xtask gen-docs` to regenerate",);
            std::process::exit(1);
        }
        println!("docs/SPEC.md is up to date");
        return Ok(());
    }

    if updated == original {
        println!("docs/SPEC.md already up to date");
        return Ok(());
    }

    fs::write(&spec_path, &updated).with_context(|| format!("writing {}", spec_path.display()))?;
    println!("regenerated {}", spec_path.display());
    Ok(())
}

/// Build the lints catalogue markdown table from `rustricted_lints::all_rules()`.
///
/// Column widths are fixed to keep diffs minimal across runs.
fn build_lints_table() -> String {
    let rules = rustricted_lints::all_rules();
    let mut out = String::new();
    out.push_str("| Code  | Name                  | Phase | Severity |\n");
    out.push_str("| ----- | --------------------- | ----- | -------- |\n");
    for rule in rules {
        out.push_str(&format!(
            "| {:<5} | {:<21} | 1     | error    |\n",
            rule.code(),
            rule.name(),
        ));
    }
    out
}

/// Replace the text between `begin` and `end` marker lines with `content`.
/// Both markers are preserved; the content is sandwiched between them with
/// a leading and trailing blank line so the result is still valid markdown.
fn replace_section(input: &str, begin: &str, end: &str, content: &str) -> Result<String> {
    let begin_idx = input
        .find(begin)
        .with_context(|| format!("missing marker: {begin}"))?;
    let end_idx = input
        .find(end)
        .with_context(|| format!("missing marker: {end}"))?;
    if end_idx < begin_idx {
        bail!("end marker `{end}` appears before begin marker `{begin}`");
    }
    let before = &input[..begin_idx + begin.len()];
    let after = &input[end_idx..];
    Ok(format!("{before}\n\n{content}\n{after}"))
}
