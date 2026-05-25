#![strict]

use std::fs;

fn main() {
    let content = match fs::read_to_string("/tmp/server_config.txt") {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error reading file: {}", e);
            return;
        }
    };

    let line = match content.lines().find(|l| !l.is_empty()) {
        Some(l) => l,
        None => {
            eprintln!("No non-empty lines found");
            return;
        }
    };

    let parts: Vec<&str> = line.split('=').collect();
    if parts.len() != 2 {
        eprintln!("Expected exactly one '=' in config line");
        return;
    }

    let key = parts[0];
    let value = parts[1];

    if key != "listen" {
        eprintln!("Expected key 'listen', got '{}'", key);
        return;
    }

    let addr_parts: Vec<&str> = value.split(':').collect();
    if addr_parts.len() != 2 {
        eprintln!("Expected exactly one ':' in listen value");
        return;
    }

    let host = addr_parts[0];
    let port_str = addr_parts[1];

    let port = match port_str.parse::<u16>() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error parsing port: {}", e);
            return;
        }
    };

    println!("host = {}, port = {}", host, port);
}
