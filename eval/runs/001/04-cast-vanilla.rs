use std::convert::TryFrom;

fn to_u32(n: u64) -> Result<u32, std::num::TryFromIntError> {
    u32::try_from(n)
}

fn main() {
    let result = to_u32(1_000_000_000u64);
    println!("{:?}", result);
}
