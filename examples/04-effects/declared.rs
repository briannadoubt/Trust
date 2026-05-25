#![strict]

fn announce(msg: &str) effect io {
    println!("{msg}");
}

fn pure_compute(x: u32) -> u32 {
    x * 2
}

fn main() effect io {
    announce(msg: "starting");
    let result = pure_compute(x: 21);
    announce(msg: "done");
    println!("{result}");
}
