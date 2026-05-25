#![strict]

fn main() {
    let x: Option<u32> = Some(1);
    let n = x.unwrap();
    println!("{n}");
}
