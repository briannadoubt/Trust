//! Eval fixture: bare `- 1` on a `.len()` value — debug panics on an empty
//! frame, release wraps to `usize::MAX`. `cargo trustc build` fails with
//! exactly R0019.

/// Number of payload bytes in a frame whose first byte is the header.
fn trailing_bytes(frame: &[u8]) -> usize {
    frame.len() - 1
}

fn main() {
    let frame = [3u8, 10, 20, 30];
    println!("payload bytes = {}", trailing_bytes(&frame));
}
