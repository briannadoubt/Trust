#![strict]

// This program declares `main` as effectless but actually performs IO via
// `println!`. Building it should fail with R4001 pointing at the missing
// `io` effect.

fn main() effect {
    println!("oops, I needed to declare io");
}
