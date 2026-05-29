#![strict]

#[derive(Debug)]
struct User {
    name: String,
    age: u32,
}

fn load_user(path: &str) -> Result<User, std::io::Error> {
    let content = std::fs::read_to_string(path)?;

    let line = content.lines().next().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "File is empty or contains no lines")
    })?;

    let (name_str, age_str) = line.split_once(',').ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "Line does not contain a comma")
    })?;

    let age = age_str.trim().parse::<u32>().map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Failed to parse age from '{}': {}", age_str.trim(), e),
        )
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