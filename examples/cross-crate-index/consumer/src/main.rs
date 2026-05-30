// A strict consumer that calls into the `producer` dependency using
// named-argument syntax. Stock rustc rejects `make_rect(width: …, height: …)`;
// the `trust-rustc` wrapper reorders it to positional against producer's
// generated signature index (RT-66) before rustc sees the file.
//
// Build it with the wrapper and the dependency index (see ../README.md):
//
//   trust index ../producer/src -o ../producer/trust-signatures.txt
//   RUSTC_WRAPPER=$(realpath …/trust-rustc) \
//   TRUST_SIGNATURE_PATH=$(realpath ../producer/trust-signatures.txt) \
//     cargo run
//
// Without TRUST_SIGNATURE_PATH the cross-crate call is unresolved: the named
// arguments are stripped in source order (so a *swap* would silently ship) and
// a positional `make_rect(1080, 1920)` would NOT fire R0042. That is exactly
// the gap this example demonstrates closing.
#![strict]

fn main() {
    // Free argument order at the call site; the wrapper reorders to the
    // declared (width, height) signature. 1920 * 1080 = 2073600.
    let area = producer::make_rect(height: 1080, width: 1920);
    println!("{area}");
}
