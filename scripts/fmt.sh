#!/usr/bin/env bash
# Run rustfmt over all workspace crates, skipping any whose source contains
# dialect syntax extensions. Rustfmt parses with rustc and rejects the pipe
# |> operator and named-arg rewrites, so strict-syntax files must be excluded.
#
# Usage:
#   bash scripts/fmt.sh          # reformat
#   bash scripts/fmt.sh --check  # check only (for CI)

set -euo pipefail

CHECK_FLAG=""
if [[ "${1:-}" == "--check" ]]; then
  CHECK_FLAG="-- --check"
fi

# Single metadata call: emit "name\tsrc_dir" pairs.
PKG_PAIRS=$(cargo metadata --no-deps --format-version 1 \
  | python3 -c "
import sys, json, os
meta = json.load(sys.stdin)
for p in meta['packages']:
    manifest = p['manifest_path']
    src_dir = os.path.join(os.path.dirname(manifest), 'src')
    print(p['name'] + '\t' + src_dir)
")

FMT_PKGS=()
while IFS=$'\t' read -r pkg src_dir; do
  if [[ -d "$src_dir" ]] && grep -r --include="*.rs" -qE '\|>' "$src_dir" 2>/dev/null; then
    echo "fmt: skip $pkg (pipe |> syntax)"
    continue
  fi
  # Skip crates that activate strict mode via `trust_attrs::strict!{}`
  # or `#![strict]` — those files use named-arg syntax that rustfmt rejects.
  if [[ -d "$src_dir" ]] && grep -r --include="*.rs" -qE '^(trust_attrs::strict\s*!|#!\[strict\])' "$src_dir" 2>/dev/null; then
    echo "fmt: skip $pkg (strict-mode dialect syntax)"
    continue
  fi
  FMT_PKGS+=("$pkg")
done <<< "$PKG_PAIRS"

if [[ ${#FMT_PKGS[@]} -eq 0 ]]; then
  echo "fmt: no packages to format"
  exit 0
fi

ARGS=()
for pkg in "${FMT_PKGS[@]}"; do
  ARGS+=(-p "$pkg")
done

# shellcheck disable=SC2086
cargo fmt "${ARGS[@]}" $CHECK_FLAG
