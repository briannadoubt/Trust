fn compute_average(values: &[u64]) -> u32 {
    if values.is_empty() {
        return 0;
    }

    let mut sum: u64 = 0;
    for &value in values {
        sum += value;
    }

    let count = values.len() as u64;
    let average = sum / count;

    average as u32
}

fn main() {
    let data = [1_000_000_000u64, 2_000_000_000, 3_000_000_000];
    let avg = compute_average(&data);
    println!("avg = {}", avg);
}