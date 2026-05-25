#![strict]

// Explicit panics drop typed error information on the floor. Return an
// `Err` so the caller can decide whether to abort.

fn divide(a: i32, b: i32) -> i32 {
    if b == 0 {
        panic!("division by zero");
    }
    a / b
}

fn main() {
    println!("{}", divide(10, 2));
}
