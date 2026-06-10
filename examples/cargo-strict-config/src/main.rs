//! No `#![strict]`, no `strict!{}` — activation comes entirely from
//! `[package.metadata.trust] strict = true` in Cargo.toml, applied by
//! `cargo trustc`. The named-argument call below is invalid stable Rust and
//! only compiles because the project-level opt-in triggers lowering.

fn make_point(x: i32, y: i32, z: i32) -> (i32, i32, i32) {
    (x, y, z)
}

fn main() {
    let p = make_point(x: 4, y: 5, z: 6);
    println!("{p:?}");
}
