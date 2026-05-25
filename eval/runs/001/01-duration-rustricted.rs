#![strict]

fn make_duration(secs: u64, nanos: u32) -> std::time::Duration {
    std::time::Duration::new(secs, nanos)
}

fn main() {
    let dur = make_duration(secs: 60, nanos: 500);
    println!("{:?}", dur);
}
