#![strict]

fn compute_average(values: &[u64]) -> u32 {
    let sum: u64 = values.iter().fold(0u64, |acc, &x| acc + x);
    let len: u64 = values.len() as u64;
    (sum / len) as u32
}

fn main() {
    let avg = compute_average(&[1_000_000_000u64, 2_000_000_000, 3_000_000_000]);
    println!("avg = {}", avg);
}
