//! Integration tests for `trust new` (RT-94) — the strict-project scaffold.
//!
//! These shell out to the binary cargo built for this test target via the
//! `CARGO_BIN_EXE_trust` env var. Each test scaffolds inside its own tempdir
//! (cleaned up on drop) so runs never collide.

use std::path::Path;
use std::process::Command;

fn trust_bin() -> &'static str {
    env!("CARGO_BIN_EXE_trust")
}

/// Run `trust new <name>` with `dir` as the working directory and return
/// (status_success, stderr).
fn run_new(dir: &Path, name: &str) -> (bool, String) {
    let output = Command::new(trust_bin())
        .args(["new", name])
        .current_dir(dir)
        .output()
        .expect("spawning trust new");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn scaffold_creates_strict_project() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (ok, stderr) = run_new(tmp.path(), "demo");
    assert!(
        ok,
        "expected `trust new demo` to succeed; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("cargo trustc run"),
        "success message should point at `cargo trustc run`, got:\n{stderr}"
    );

    let root = tmp.path().join("demo");
    for file in [
        "Cargo.toml",
        "src/main.rs",
        ".gitignore",
        "README.md",
        "CLAUDE.md",
    ] {
        assert!(root.join(file).is_file(), "missing scaffolded file {file}");
    }

    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(
        manifest.contains("name = \"demo\""),
        "manifest:\n{manifest}"
    );
    assert!(
        manifest.contains("[package.metadata.trust]") && manifest.contains("strict = true"),
        "manifest must opt into strict mode:\n{manifest}"
    );
    assert!(
        !manifest.contains("[workspace]"),
        "scaffolded projects are standalone; manifest:\n{manifest}"
    );

    let main_rs = std::fs::read_to_string(root.join("src/main.rs")).unwrap();
    assert!(
        main_rs.contains("make_point(x: 1, y: 2)"),
        "main.rs must exercise named-argument syntax:\n{main_rs}"
    );

    let gitignore = std::fs::read_to_string(root.join(".gitignore")).unwrap();
    assert!(gitignore.contains("/target"), "gitignore:\n{gitignore}");

    let readme = std::fs::read_to_string(root.join("README.md")).unwrap();
    assert!(
        readme.contains("cargo trustc build") && readme.contains("cargo trustc run"),
        "README must use the cargo trustc commands:\n{readme}"
    );

    let claude_md = std::fs::read_to_string(root.join("CLAUDE.md")).unwrap();
    assert!(
        claude_md.contains("cargo trustc build"),
        "CLAUDE.md must mention `cargo trustc build`:\n{claude_md}"
    );
}

#[test]
fn scaffold_refuses_existing_directory() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(tmp.path().join("taken")).unwrap();

    let (ok, stderr) = run_new(tmp.path(), "taken");
    assert!(!ok, "expected failure for an existing directory");
    assert!(
        stderr.contains("already exists"),
        "stderr should explain the conflict, got:\n{stderr}"
    );
}

#[test]
fn scaffold_rejects_invalid_names() {
    let tmp = tempfile::tempdir().expect("tempdir");
    for bad in ["9lives", "has space", "no/slash"] {
        let (ok, stderr) = run_new(tmp.path(), bad);
        assert!(!ok, "expected `trust new {bad}` to fail");
        assert!(
            stderr.contains("invalid project name") || stderr.contains("must not"),
            "stderr for `{bad}` should explain the rejection, got:\n{stderr}"
        );
        assert!(
            !tmp.path().join(bad).exists(),
            "no directory should be created for `{bad}`"
        );
    }
}
