trust_attrs::strict! {}

// RT-32 regression fixture: named-arg call to a function defined in another
// crate (`trust-std`). Without cross-crate seeding the call site
// `trust_std::fs::write_text(path: ..., contents: ...)` is passed
// through to rustc unchanged, which then rejects it as a label-style call.
// With seeding, the lowering pass rewrites it to positional form against
// the real signature (`path`, `contents`) — including reordering when the
// caller supplies the args out of declaration order.

use std::path::PathBuf;

fn main() {
    let tmp_dir = std::env::temp_dir();
    let path: PathBuf = tmp_dir.join("trust-cross-crate.txt");

    // Args intentionally supplied in reverse order to exercise the
    // reorder path.
    trust_std::fs::write_text(contents: "ok\n", path: &path)
        .expect("write_text");

    let read = trust_std::fs::read_to_string(path: &path)
        .expect("read_to_string");

    let _ = trust_std::fs::remove_file(path: &path);

    print!("{read}");
}
