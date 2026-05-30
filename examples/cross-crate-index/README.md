# Cross-crate named arguments (RT-66)

This example closes the dialect's last big coverage gap: enforcing named
arguments (R0042) on calls into a **third-party crate**, resolved against a
signature index extracted automatically from that crate's source — no
hand-written `trust-std`-style shim.

```
producer/   plain-Rust dependency exposing `make_rect(width, height)`
consumer/   strict crate that calls `producer::make_rect(...)` by name
```

## Run it

From this directory, with the toolchain built (`cargo build -p trust -p trust-rustc`):

```sh
TRUST=$(realpath ../../target/debug/trust)
WRAPPER=$(realpath ../../target/debug/trust-rustc)

# 1. Auto-extract producer's public-fn signatures (no hand-written shim).
"$TRUST" index producer/src -o producer/trust-signatures.txt

# 2. Build the consumer with the wrapper + the dependency index.
cd consumer
RUSTC_WRAPPER="$WRAPPER" \
TRUST_SIGNATURE_PATH="$(realpath ../producer/trust-signatures.txt)" \
  cargo run        # prints 2073600  (1920 * 1080)
```

The named call `producer::make_rect(height: 1080, width: 1920)` lowers to the
positional `producer::make_rect(1920, 1080)` — reordered into producer's
declared `(width, height)` order before rustc sees it.

## The gap, demonstrated

Swap `consumer/src/main.rs` to a positional swap and check it:

```sh
# Positional call into the dependency with the arguments swapped.
printf '#![strict]\nfn main() { let _ = producer::make_rect(1080, 1920); }\n' > /tmp/swap.rs

# WITHOUT the index: silently accepted — the bug ships.
"$TRUST" check /tmp/swap.rs                                   # ok

# WITH the index: rejected with R0042 and a literal fix.
TRUST_SIGNATURE_PATH="$(realpath producer/trust-signatures.txt)" \
  "$TRUST" check /tmp/swap.rs                                 # error[R0042]
```

`TRUST_SIGNATURE_PATH` accepts a single manifest, a `:`-separated list, or a
directory of `*.txt` manifests — one per dependency you want enforced.

## What's still manual

You generate the indices and point the build at them. Fully automatic
discovery of every dependency's index from cargo's build graph (so no env var
is needed) is the remaining ergonomics step; the extraction and enforcement
themselves are done.
