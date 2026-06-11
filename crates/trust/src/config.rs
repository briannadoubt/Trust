//! Project configuration for `trust check` (RT-102): a `trust.toml` that
//! selects the rule set and lists project-wide suppressions/relaxations, so
//! adopters don't sprinkle inline `#[allow(trust::Rxxxx)]` across every call
//! site. The first external adopter would have needed 162 inline allows for one
//! rule alone; this is the coarse, one-place sibling of that.
//!
//! ```toml
//! # trust.toml — at the project root
//! rules = "bugs"          # base selection: all | bugs | safety | "R0001,R0003,…"
//! allow = ["R0012"]       # dropped project-wide (never reported)
//! warn  = ["R0017"]       # kept, but downgraded to a non-failing warning
//! ```
//!
//! The CLI `--rules` flag overrides `rules`. Severity downgrades let a project
//! adopt a rule as advisory-only without it failing CI.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use trust_diag::{Diagnostic, Severity};

#[derive(Debug, Default)]
pub struct TrustConfig {
    /// Base rule selection (`all` | `bugs` | `safety` | a code list). `None`
    /// when unset; the CLI `--rules` flag, if given, takes precedence.
    pub rules: Option<String>,
    /// Rule codes suppressed project-wide — a diagnostic with one of these
    /// codes is dropped everywhere.
    pub allow: Vec<String>,
    /// Rule codes downgraded to warnings project-wide — still reported, but
    /// they don't make `check` exit non-zero.
    pub warn: Vec<String>,
}

impl TrustConfig {
    /// Discover and load the nearest `trust.toml` at or above `start`, walking
    /// toward the filesystem root. `start` may be a file or a directory. Returns
    /// an empty (no-op) config when none is found, so callers never branch on
    /// "is there a config".
    pub fn discover(start: &Path) -> Result<TrustConfig> {
        let start_dir: PathBuf = if start.is_dir() {
            start.to_path_buf()
        } else if let Some(p) = start.parent().filter(|p| !p.as_os_str().is_empty()) {
            p.to_path_buf()
        } else {
            // Bare filename or stdin (`-`): search from the working directory.
            std::env::current_dir().context("resolving current directory")?
        };

        let mut cur: Option<&Path> = Some(&start_dir);
        while let Some(dir) = cur {
            let candidate = dir.join("trust.toml");
            if candidate.is_file() {
                return Self::load(&candidate);
            }
            cur = dir.parent();
        }
        Ok(TrustConfig::default())
    }

    fn load(path: &Path) -> Result<TrustConfig> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let value: toml::Value = text
            .parse()
            .with_context(|| format!("parsing {}", path.display()))?;

        let rules = value
            .get("rules")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let allow = code_list(&value, "allow", path)?;
        let warn = code_list(&value, "warn", path)?;

        Ok(TrustConfig { rules, allow, warn })
    }

    /// Apply project-level `allow`/`warn` to a computed diagnostic set: drop the
    /// suppressed codes, downgrade the relaxed ones to warnings.
    pub fn apply(&self, diags: &mut Vec<Diagnostic>) {
        if !self.allow.is_empty() {
            diags.retain(|d| !self.allow.iter().any(|c| c.as_str() == d.rule));
        }
        if !self.warn.is_empty() {
            for d in diags.iter_mut() {
                if self.warn.iter().any(|c| c.as_str() == d.rule) {
                    d.severity = Severity::Warning;
                }
            }
        }
    }
}

/// Parse a `key = ["R0001", …]` array of rule codes, validating each against
/// the registry so a typo fails loudly rather than silently doing nothing.
/// Codes are upper-cased so `r0001` and `R0001` both work.
fn code_list(value: &toml::Value, key: &str, path: &Path) -> Result<Vec<String>> {
    let items = match value.get(key) {
        None => return Ok(Vec::new()),
        Some(toml::Value::Array(items)) => items,
        Some(_) => bail!("`{key}` in {} must be an array of rule codes", path.display()),
    };
    let mut out = Vec::with_capacity(items.len());
    for it in items {
        let code = it
            .as_str()
            .with_context(|| format!("`{key}` in {} must be a list of strings", path.display()))?
            .to_uppercase();
        if trust_lints::Rule::from_code(&code).is_none() {
            bail!(
                "unknown rule code `{code}` in {} (`{key}`) — run `trust explain` for the catalogue",
                path.display()
            );
        }
        out.push(code);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diag(code: &'static str) -> Diagnostic {
        Diagnostic::error(code, "x", 0..1)
    }

    #[test]
    fn discover_walks_up_and_parses() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("trust.toml"),
            "rules = \"bugs\"\nallow = [\"r0001\"]\nwarn = [\"R0003\"]\n",
        )
        .unwrap();
        let deep = root.join("crates/foo/src");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("lib.rs"), "fn a() {}").unwrap();

        // Found from a file nested several levels below the config.
        let cfg = TrustConfig::discover(&deep.join("lib.rs")).unwrap();
        assert_eq!(cfg.rules.as_deref(), Some("bugs"));
        assert_eq!(cfg.allow, vec!["R0001"]); // upper-cased
        assert_eq!(cfg.warn, vec!["R0003"]);
    }

    #[test]
    fn discover_returns_default_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = TrustConfig::discover(dir.path()).unwrap();
        assert!(cfg.rules.is_none() && cfg.allow.is_empty() && cfg.warn.is_empty());
    }

    #[test]
    fn unknown_code_is_a_loud_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("trust.toml"), "allow = [\"R9999\"]\n").unwrap();
        let err = TrustConfig::discover(dir.path()).unwrap_err();
        assert!(err.to_string().contains("R9999"));
    }

    #[test]
    fn apply_drops_allowed_and_downgrades_warned() {
        let cfg = TrustConfig {
            rules: None,
            allow: vec!["R0001".into()],
            warn: vec!["R0003".into()],
        };
        let mut diags = vec![diag("R0001"), diag("R0003"), diag("R0014")];
        cfg.apply(&mut diags);
        // R0001 dropped; R0003 downgraded; R0014 untouched.
        assert!(!diags.iter().any(|d| d.rule == "R0001"));
        let r0003 = diags.iter().find(|d| d.rule == "R0003").unwrap();
        assert!(matches!(r0003.severity, Severity::Warning));
        let r0014 = diags.iter().find(|d| d.rule == "R0014").unwrap();
        assert!(matches!(r0014.severity, Severity::Error));
    }
}
