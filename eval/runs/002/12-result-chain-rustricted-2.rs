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
    let first_line = contents.lines().next().ok_or_else(|| {
        io::Error::new(ErrorKind::InvalidData, "file is empty")
    })?;

    let mut parts = first_line.split(',');
    let name = parts.next().ok_or_else(|| {
        io::Error::new(ErrorKind::InvalidData, "missing name field")
    })?.to_string();
    let age_str = parts.next().ok_or_else(|| {
        io::Error::new(ErrorKind::InvalidData, "missing age field")
    })?;

    if parts.next().is_some() {
        return Err(io::Error::new(ErrorKind::InvalidData, "too many fields"));
    }

    let age = age_str.trim().parse::<u32>().map_err(|_| {
        io::Error::new(ErrorKind::InvalidData, "failed to parse age")
    })?;

    Ok(User { name, age })
}

fn main() {
    let result = load_user("/tmp/eval_user.txt");
    println!("{:?}", result);
}
