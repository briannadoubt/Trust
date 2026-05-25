#![strict]

use std::fs::File;
use std::io::{self, BufRead, BufReader, ErrorKind};

#[derive(Debug)]
struct User {
    name: String,
    age: u32,
}

fn load_user(path: &str) -> Result<User, std::io::Error> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let first_line = reader
        .lines()
        .next()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "file is empty"))??;

    let mut parts = first_line.split(',');
    let name = parts
        .next()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "missing name"))?
        .to_string();
    let age_str = parts
        .next()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "missing age"))?;

    if parts.next().is_some() {
        return Err(io::Error::new(ErrorKind::InvalidData, "too many fields"));
    }

    let age = age_str
        .trim()
        .parse::<u32>()
        .map_err(|_| io::Error::new(ErrorKind::InvalidData, "age is not a valid u32"))?;

    Ok(User { name, age })
}

fn main() {
    let result = load_user("/tmp/eval_user.txt");
    println!("{:?}", result);
}
