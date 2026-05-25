#![strict]

use std::fs;

fn main() {
    let config_content = fs::read_to_string("/tmp/server_config.txt").unwrap_or_default();

    let first_line = config_content
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");

    let parts: Vec<&str> = first_line.split('=').collect();
    if parts.len() != 2 {
        return;
    }

    let key = parts[0].trim();
    if key != "listen" {
        return;
    }

    let value = parts[1].trim();
    let addr_parts: Vec<&str> = value.split(':').collect();
    if addr_parts.len() != 2 {
        return;
    }

    let host = addr_parts[0];
    let port_str = addr_parts[1];

    let port: u16 = match port_str.parse() {
        Ok(p) => p,
        Err(_) => return,
    };

    println!("host = {}, port = {}", host, port);
}
