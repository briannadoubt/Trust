#![strict]

// `area` takes two parameters, so the call site must name them in strict
// mode. The R0042 lint catches the positional call below.

fn area(width: u32, height: u32) -> u32 {
    width * height
}

fn main() {
    let a = area(4, 6);
    println!("{a}");
}
