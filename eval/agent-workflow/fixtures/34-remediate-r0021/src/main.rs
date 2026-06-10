//! Eval fixture: `.capacity()` used as a loop bound — capacity sizes future
//! allocations, it is not the element count, so the loop visits slots that
//! were never written. `cargo trust build` fails with exactly R0021.

fn checksum(buf: &Vec<u8>) -> u32 {
    let mut sum = 0u32;
    for i in 0..buf.capacity() {
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
