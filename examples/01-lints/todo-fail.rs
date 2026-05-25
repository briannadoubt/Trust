#![strict]

// `todo!()` ships as a runtime panic; in strict mode it must either be
// implemented or fenced behind `#[cfg(test)]`.

fn compute_total(x: u32, y: u32) -> u32 {
    todo!()
}

fn main() {
    let n = compute_total(1, 2);
    println!("{n}");
}
