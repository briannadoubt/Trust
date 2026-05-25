use std::io::{self, BufRead, Error, ErrorKind};

#[derive(Debug)]
struct User {
    name: String,
    age: u32,
}

fn load_user(path: &str) -> Result<User, Error> {
    let file = std::fs::File::open(path)?;
    let mut lines = io::BufReader::new(file).lines();
    let line = lines
        .next()
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "file is empty"))??;
    let mut parts = line.splitn(2, ',');
    let name = parts
        .next()
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "missing name"))?
        .to_string();
    let age_str = parts
        .next()
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "missing age"))?;
    let age = age_str
        .trim()
        .parse::<u32>()
        .map_err(|e| Error::new(ErrorKind::InvalidData, e))?;
    Ok(User { name, age })
}

fn main() {
    let result = load_user("/tmp/eval_user.txt");
    println!("{:?}", result);
}
