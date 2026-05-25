fn area(width: u32, height: u32) -> u32 {
    width * height
}

fn main() {
    let a = area(width: 4, height: 6);
    let b = area(height: 6, width: 4);
    println!("{a} {b}");
}
