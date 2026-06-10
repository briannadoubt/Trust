#!/usr/bin/env python3
"""
Drive `trust check` against every .rs file in:
  1) workspace crates under /Users/bri/dev/One/crates
  2) external crate /tmp/anyhow-1.0.86

For each file we:
  * read source
  * write a temp file with `#![strict]` prepended (preserving any
    shebang or doc-comment leading lines if needed -- syn handles inner
    attributes anywhere at the top, but we just prepend for simplicity)
  * run `trust check` and capture stdout+stderr
  * parse diagnostics out of the ANSI-coloured output using a regex

Output: writes a JSON file per target with per-file diagnostic records,
and a final report.md aggregating the findings.

We classify TP/FP manually in a second pass by reading per-rule samples,
then write the final REPORT.md.
"""

import json
import os
import re
import subprocess
import sys
import tempfile
from collections import defaultdict
from pathlib import Path

TRUST = "/Users/bri/dev/One/target/debug/trust"
OUT_DIR = Path("/Users/bri/dev/One/eval/false-positives")

# Strip ANSI escape sequences so the regex can find the rule code reliably.
ANSI_RE = re.compile(r"\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])")

# We only need rule code + the primary message line.
# Format example (post-strip): "[R0001] Error: `.unwrap()` is banned in strict mode"
DIAG_HEADER_RE = re.compile(r"\[(R\d{4})\]\s+Error:\s+(.+)")

# Span line example: " ╭─[/tmp/test_strict.rs:6:33]"
SPAN_RE = re.compile(r"╭─\[(.+?):(\d+):(\d+)\]")


def strip_ansi(text: str) -> str:
    return ANSI_RE.sub("", text)


def inject_strict(src: str) -> str:
    """Prepend `#![strict]` to the source.

    A pre-existing `#![strict]` is left as-is.  If the file already has
    a `#![...]` inner attribute we insert after the shebang (if any)
    but before that attribute -- syn accepts multiple inner attributes
    in any order, but `#![strict]` must be present for the lints to
    activate.
    """
    if "#![strict]" in src:
        return src
    lines = src.splitlines(keepends=True)
    # Preserve shebang if present
    insert_at = 0
    if lines and lines[0].startswith("#!") and not lines[0].startswith("#!["):
        insert_at = 1
    return "".join(lines[:insert_at]) + "#![strict]\n" + "".join(lines[insert_at:])


def run_check(src: str) -> tuple[str, int]:
    """Write src to a tmp file and run trust check.  Returns (output, exit_code)."""
    with tempfile.NamedTemporaryFile(
        mode="w", suffix=".rs", delete=False, dir="/tmp"
    ) as f:
        f.write(src)
        tmp_path = f.name
    try:
        proc = subprocess.run(
            [TRUST, "check", tmp_path],
            capture_output=True,
            text=True,
            timeout=60,
        )
        out = proc.stdout + proc.stderr
        return out, proc.returncode
    finally:
        os.unlink(tmp_path)


def parse_diagnostics(output: str, source: str) -> list[dict]:
    """Parse trust check output into structured diagnostics.

    Each diagnostic block looks like:
        [R0001] Error: <message>
           ╭─[<path>:<line>:<col>]
           │
         N │ <source line>
           │  ──┬──
           ...
    """
    clean = strip_ansi(output)
    lines = clean.splitlines()
    diagnostics = []
    src_lines = source.splitlines()

    i = 0
    while i < len(lines):
        m = DIAG_HEADER_RE.search(lines[i])
        if not m:
            i += 1
            continue
        rule = m.group(1)
        msg = m.group(2).strip()
        # Look ahead for span info
        line_no, col_no = None, None
        snippet = ""
        for j in range(i + 1, min(i + 6, len(lines))):
            sm = SPAN_RE.search(lines[j])
            if sm:
                try:
                    line_no = int(sm.group(2))
                    col_no = int(sm.group(3))
                except ValueError:
                    pass
                break
        if line_no is not None and 1 <= line_no <= len(src_lines):
            snippet = src_lines[line_no - 1].strip()
        diagnostics.append(
            {
                "rule": rule,
                "message": msg,
                "line": line_no,
                "col": col_no,
                "snippet": snippet,
            }
        )
        i += 1
    return diagnostics


def collect_files(roots: list[str], skip: set[str] | None = None) -> list[Path]:
    skip = skip or set()
    out = []
    for root in roots:
        p = Path(root)
        if p.is_file():
            out.append(p)
            continue
        for path in p.rglob("*.rs"):
            if any(seg in skip for seg in path.parts):
                continue
            out.append(path)
    return sorted(out)


def process(label: str, files: list[Path]) -> dict:
    """Run check on each file, return aggregated report dict."""
    print(f"\n=== {label}: {len(files)} files ===", file=sys.stderr)
    per_file = []
    for path in files:
        try:
            src = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError) as e:
            print(f"  SKIP {path}: {e}", file=sys.stderr)
            continue
        strict_src = inject_strict(src)
        out, rc = run_check(strict_src)
        diags = parse_diagnostics(out, strict_src)
        per_file.append(
            {
                "path": str(path),
                "exit_code": rc,
                "diagnostic_count": len(diags),
                "diagnostics": diags,
            }
        )
        if diags:
            print(f"  {path.name}: {len(diags)} diag(s)", file=sys.stderr)
    return {"label": label, "files": per_file}


def main() -> None:
    workspace_roots = [
        "/Users/bri/dev/One/crates/cargo-trustc",
        "/Users/bri/dev/One/crates/trust",
        "/Users/bri/dev/One/crates/trust-diag",
        "/Users/bri/dev/One/crates/trust-effects",
        "/Users/bri/dev/One/crates/trust-lints",
        "/Users/bri/dev/One/crates/trust-lower",
        "/Users/bri/dev/One/crates/trust-lsp",
        "/Users/bri/dev/One/crates/trust-std",
        "/Users/bri/dev/One/crates/trust-syntax",
        "/Users/bri/dev/One/crates/xtask",
    ]
    workspace_files = collect_files(workspace_roots)
    workspace_report = process("workspace", workspace_files)

    external_files = collect_files(
        ["/tmp/anyhow-1.0.86/src", "/tmp/anyhow-1.0.86/tests"],
        skip={"ui"},  # ui/ tests are meant to fail compilation; not useful
    )
    external_report = process("anyhow-1.0.86", external_files)

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    (OUT_DIR / "raw_workspace.json").write_text(
        json.dumps(workspace_report, indent=2)
    )
    (OUT_DIR / "raw_anyhow.json").write_text(json.dumps(external_report, indent=2))

    # Aggregate by rule
    summary = {"workspace": {}, "anyhow-1.0.86": {}, "totals": {}}
    for label, report in [("workspace", workspace_report), ("anyhow-1.0.86", external_report)]:
        per_rule = defaultdict(int)
        for f in report["files"]:
            for d in f["diagnostics"]:
                per_rule[d["rule"]] += 1
        summary[label] = dict(per_rule)
    totals = defaultdict(int)
    for label in ("workspace", "anyhow-1.0.86"):
        for rule, count in summary[label].items():
            totals[rule] += count
    summary["totals"] = dict(totals)
    (OUT_DIR / "summary.json").write_text(json.dumps(summary, indent=2))

    print("\n=== summary by rule ===", file=sys.stderr)
    for rule in sorted(set(summary["totals"].keys())):
        w = summary["workspace"].get(rule, 0)
        a = summary["anyhow-1.0.86"].get(rule, 0)
        print(f"  {rule}: workspace={w}, anyhow={a}, total={w + a}", file=sys.stderr)


if __name__ == "__main__":
    main()
