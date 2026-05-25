#![strict]

use std::fs::File;
use std::io::{BufRead, BufReader, Error, ErrorKind};

#[derive(Debug)]
struct User {
    name: String,
    age: u32,
}

fn load_user(path: &str) -> Result<User, std::io::Error> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    let first_line = lines
        .next()
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "No lines in file"))??;

    let parts: Vec<&str> = first_line.split(',').collect();

    if parts.len() != 2 {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "Expected exactly two comma-separated values",
        ));
    }

    let name = parts[0].to_string();
    let age = parts[1].parse::<u32>().map_err(|_| {
        Error::new(ErrorKind::InvalidData, "Failed to parse age as u32")
    })?;

    Ok(User { name, age })
}

fn main() {
    let result = load_user("/tmp/eval_user.txt");
    println!("{:?}", result);
}
