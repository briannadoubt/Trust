use std::fs;

fn load_config(path: &str) -> Result<String, std::io::Error> {
    fs::read_to_string(path)
}

fn main() -> Result<(), std::io::Error> {
    let config = load_config("/etc/hosts")?;
    let preview = if config.len() > 50 {
        &config[..50]
    } else {
        &config
    };
    println!("{}", preview);
    Ok(())
}
