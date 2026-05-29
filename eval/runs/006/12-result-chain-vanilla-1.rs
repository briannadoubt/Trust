use std::fs;
use std::io::{self, ErrorKind};

#[derive(Debug)]
struct User {
    name: String,
    age: u32,
}

fn load_user(path: &str) -> Result<User, io::Error> {
    let content = fs::read_to_string(path)?;

    let first_line = content.lines().next().ok_or_else(|| {
        io::Error::new(ErrorKind::InvalidData, "File is empty or has no lines")
    })?;

    let (name_str, age_str) = first_line.split_once(',').ok_or_else(|| {
        io::Error::new(ErrorKind::InvalidData, "First line does not contain a comma separator")
    })?;

    let age = age_str.trim().parse::<u32>().map_err(|e| {
        io::Error::new(ErrorKind::InvalidData, format!("Failed to parse age: {}", e))
    })?;

    Ok(User {
        name: name_str.to_string(),
        age,
    })
}

fn main() {
    let result = load_user("/tmp/eval_user.txt");
    println!("{:?}", result);
}