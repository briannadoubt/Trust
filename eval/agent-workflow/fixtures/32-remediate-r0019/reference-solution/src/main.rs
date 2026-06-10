//! REFERENCE SOLUTION — never shown to the agent. `saturating_sub` makes the
//! empty-frame behavior explicit (0 payload bytes), per the R0019 `instead:`
//! guidance. `cargo trust build` is green.

/// Number of payload bytes in a frame whose first byte is the header.
/// An empty frame has zero payload bytes.
fn trailing_bytes(frame: &[u8]) -> usize {
    frame.len().saturating_sub(1)
}

fn main() {
    let frame = [3u8, 10, 20, 30];
    println!("payload bytes = {}", trailing_bytes(&frame));
}
