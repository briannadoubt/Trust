#![strict]

use std::fs;
use std::io::{self, ErrorKind};

#[derive(Debug)]
struct User {
    name: String,
    age: u32,
}

fn load_user(path: &str) -> Result<User, io::Error> {
    let contents = fs::read_to_string(path)?;
    let first_line = contents
        .lines()
        .next()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "file is empty"))?;
    let mut parts = first_line.splitn(2, ',');
    let name = parts
        .next()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "missing name field"))?
        .to_string();
    let age_str = parts
        .next()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "missing age field"))?
        .trim();
    let age = age_str
        .parse::<u32>()
        .map_err(|e| io::Error::new(ErrorKind::InvalidData, e))?;
    Ok(User { name, age })
}

fn main() {
    let result = load_user("/tmp/eval_user.txt");
    println!("{:?}", result);
}
