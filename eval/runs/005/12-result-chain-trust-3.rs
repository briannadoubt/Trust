#![strict]

use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

#[derive(Debug)]
struct User {
    name: String,
    age: u32,
}

fn load_user(path: &str) -> Result<User, std::io::Error> {
    let file = File::open(path)?;
    let mut lines = BufReader::new(file).lines();

    if let Some(Ok(line)) = lines.next() {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() == 2 {
            let name = parts[0].to_string();
            let age_str = parts[1];
            if let Ok(age) = age_str.parse::<u32>() {
                return Ok(User { name, age });
            }
        }
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid data format"));
    }

    Err(io::Error::new(io::ErrorKind::InvalidData, "No data found"))
}

fn main() {
    let result = load_user("/tmp/eval_user.txt");
    println!("{:?}", result);
}