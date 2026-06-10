//! REFERENCE SOLUTION — never shown to the agent. The loop bound is now
//! `.len()` — the element count — per the R0021 `instead:` guidance.
//! `cargo trust build` is green.

fn checksum(buf: &Vec<u8>) -> u32 {
    let mut sum = 0u32;
    for i in 0..buf.len() {
        if let Some(byte) = buf.get(i) {
            sum = sum.wrapping_add(u32::from(*byte));
        }
    }
    sum
}

fn main() {
    let mut buf = Vec::with_capacity(16);
    buf.extend_from_slice(&[1u8, 2, 3]);
    println!("checksum = {}", checksum(&buf));
}
