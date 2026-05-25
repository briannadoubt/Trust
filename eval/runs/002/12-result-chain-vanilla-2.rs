use std::fs;
use std::io::{self, ErrorKind};

#[derive(Debug)]
struct User {
    name: String,
    age: u32,
}

fn load_user(path: &str) -> Result<User, std::io::Error> {
    let content = fs::read_to_string(path)?;
    let first_line = content.lines().next().ok_or_else(|| {
        io::Error::new(ErrorKind::InvalidData, "File is empty")
    })?;

    let parts: Vec<&str> = first_line.split(',').collect();
    if parts.len() != 2 {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "Expected exactly two comma-separated values",
        ));
    }

    let name = parts[0].to_string();
    let age: u32 = parts[1].trim().parse().map_err(|_| {
        io::Error::new(ErrorKind::InvalidData, "Failed to parse age as u32")
    })?;

    Ok(User { name, age })
}

fn main() {
    let result = load_user("/tmp/eval_user.txt");
    println!("{:?}", result);
}
