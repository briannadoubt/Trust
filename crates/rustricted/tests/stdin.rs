//! Integration tests for stdin (`-`) input support in the `rustricted` CLI.
//!
//! These shell out to the binary cargo built for this test target via the
//! `CARGO_BIN_EXE_rustricted` env var, so no extra dev-dependency is needed.

use std::io::Write;
use std::process::{Command, Stdio};

fn rustricted_bin() -> &'static str {
    env!("CARGO_BIN_EXE_rustricted")
}

/// Spawn the CLI with the given args, write `stdin_input` to its stdin, and
/// return (status_success, stdout, stderr).
fn run_with_stdin(args: &[&str], stdin_input: &str) -> (bool, String, String) {
    let mut child = Command::new(rustricted_bin())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawning rustricted");

    child
        .stdin
        .as_mut()
        .expect("stdin handle")
        .write_all(stdin_input.as_bytes())
        .expect("writing stdin");

    let output = child.wait_with_output().expect("waiting on rustricted");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn check_accepts_stdin_for_valid_source() {
    let src = "fn main() { let _ = 1 + 1; }\n";
    let (ok, _stdout, stderr) = run_with_stdin(&["check", "-"], src);
    assert!(ok, "expected check to succeed; stderr:\n{stderr}");
    assert!(
        stderr.contains("<stdin>"),
        "expected <stdin> label in stderr, got:\n{stderr}"
    );
}

#[test]
fn check_stdin_surfaces_lint_diagnostics() {
    // `#![strict]` + a positional 3-arg call should fire R0042 (named-args rule).
    let src = "#![strict]\nfn f(a: i32, b: i32, c: i32) -> i32 { a + b + c }\nfn main() { let _ = f(1, 2, 3); }\n";
    let (ok, _stdout, stderr) = run_with_stdin(&["check", "-"], src);
    assert!(
        !ok,
        "expected check to fail under strict mode; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("<stdin>"),
        "expected diagnostics to label source as <stdin>, got:\n{stderr}"
    );
}

#[test]
fn build_stdin_requires_out_flag() {
    let src = "fn main() {}\n";
    let (ok, _stdout, stderr) = run_with_stdin(&["build", "-"], src);
    assert!(!ok, "expected build - without --out to fail");
    assert!(
        stderr.contains("--out"),
        "expected error to mention --out; got:\n{stderr}"
    );
}

#[test]
fn build_stdin_with_out_produces_runnable_binary() {
    let tmp = tempfile::Builder::new()
        .prefix("rustricted-stdin-")
        .tempdir()
        .expect("tempdir");
    let out_path = tmp.path().join("hello");

    let src = "fn main() { println!(\"hi from stdin\"); }\n";
    let (ok, _stdout, stderr) = run_with_stdin(
        &[
            "build",
            "-",
            "--out",
            out_path.to_str().expect("utf-8 path"),
        ],
        src,
    );
    assert!(ok, "expected build to succeed; stderr:\n{stderr}");
    assert!(out_path.exists(), "expected output binary at {out_path:?}");

    let run = Command::new(&out_path)
        .output()
        .expect("running built binary");
    assert!(run.status.success(), "built binary exited non-zero");
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "hi from stdin",
        "unexpected stdout from built binary"
    );
}
