#![strict]

fn compute_average(values: &[u64]) -> u32 {
    if values.is_empty() {
        // An average of an empty set is undefined.
        // Returning 0 is a common convention for primitive return types
        // when no error handling (like Option or Result) is specified.
        return 0;
    }

    // Sum the values. Using u64 for the sum to prevent overflow,
    // as individual values are u64 and their sum can exceed u32::MAX.
    let sum: u64 = values.iter().sum();

    // Get the length of the slice. `len()` returns `usize`.
    // Cast to `u64` for division with the `u64` sum.
    let len = values.len() as u64;

    // Perform integer division to get the average as u64.
    let avg_u64 = sum / len;

    // Convert the u64 average to u32.
    // `#![strict]` often flags direct `as` casts that might truncate.
    // `try_into().expect()` provides an explicit conversion that will panic
    // if the value does not fit into u32, preventing silent truncation.
    avg_u64.try_into().expect("Average value exceeds u32 maximum")
}

fn main() {
    // Define the input values as a slice of u64.
    let values = &[1_000_000_000u64, 2_000_000_000, 3_000_000_000];

    // Call the compute_average function.
    let avg = compute_average(values);

    // Print the result.
    println!("avg = {}", avg);
}