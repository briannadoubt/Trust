#![strict]

// Public-API `bool` parameters are positional footguns even with named args
// — `spawn(detached: true)` doesn't say what `true` means. Use a named
// enum so the call site reads as `Detached::Yes`.

pub fn spawn(detached: bool, inherit_env: bool) {
    let _ = (detached, inherit_env);
}

fn main() {
    spawn(true, false);
}
