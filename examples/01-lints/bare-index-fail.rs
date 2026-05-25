#![strict]

// `v[i]` panics on out-of-bounds. `.get(i)` returns `Option<&T>` and forces
// the call site to handle the missing case.

fn first_or_zero(v: &[u32], i: usize) -> u32 {
    v[i]
}

fn main() {
    let xs = vec![10u32, 20, 30];
    println!("{}", first_or_zero(&xs, 1));
}
