#![strict]

use std::fs;

fn main() {
    let content = match fs::read_to_string("/tmp/server_config.txt") {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading file: {}", e);
            return;
        }
    };

    let first_line = match content.lines().find(|line| !line.trim().is_empty()) {
        Some(line) => line,
        None => {
            eprintln!("No non-empty lines found");
            return;
        }
    };

    let parts: Vec<&str> = first_line.split('=').collect();
    if parts.len() != 2 {
        eprintln!("Invalid config format: expected key=value");
        return;
    }

    let key = parts[0].trim();
    let value = parts[1].trim();

    if key != "listen" {
        eprintln!("Invalid key: expected 'listen', got '{}'", key);
        return;
    }

    let addr_parts: Vec<&str> = value.split(':').collect();
    if addr_parts.len() != 2 {
        eprintln!("Invalid listen format: expected host:port");
        return;
    }

    let host = addr_parts[0];
    let port_str = addr_parts[1];

    let port = match port_str.parse::<u16>() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Invalid port number: {}", e);
            return;
        }
    };

    println!("host = {}, port = {}", host, port);
}
