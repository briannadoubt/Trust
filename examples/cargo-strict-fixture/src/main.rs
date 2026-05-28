//! End-to-end fixture for the `trust-rustc` RUSTC_WRAPPER. This
//! file uses named-argument syntax (`make_point(x: 1, y: 2, z: 3)`)
//! which is invalid stable Rust — without the wrapper, `cargo build`
//! fails. With `RUSTC_WRAPPER=<path>/trust-rustc cargo build`,
//! the wrapper lowers the file into plain positional Rust before
//! handing it to the real rustc, and the build succeeds.

trust_attrs::strict! {}

fn make_point(x: i32, y: i32, z: i32) -> (i32, i32, i32) {
    (x, y, z)
}

fn main() {
    let p = make_point(x: 1, y: 2, z: 3);
    println!("{p:?}");
}
