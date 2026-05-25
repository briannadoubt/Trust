#![strict]

use std::fs;

fn load_config(path: &str) -> Result<String, std::io::Error> {
    fs::read_to_string(path)
}

fn main() -> Result<(), std::io::Error> {
    let content = load_config("/etc/hosts")?;
    let preview = if content.len() > 50 {
        &content[..50]
    } else {
        &content
    };
    println!("{}", preview);
    Ok(())
}
