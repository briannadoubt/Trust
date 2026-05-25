#![strict]

// `impl Iterator<...>` in return position hides the concrete type. Strict
// mode forces a named alias so callers (and the agent reading the code)
// can see what's actually flowing back.

fn one_to_three() -> impl Iterator<Item = u32> {
    [1u32, 2, 3].into_iter()
}

fn main() {
    let total: u32 = one_to_three().sum();
    println!("{total}");
}
