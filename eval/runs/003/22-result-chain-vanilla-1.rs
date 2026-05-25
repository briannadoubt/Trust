use std::fs;

fn main() {
    let content = fs::read_to_string("/tmp/server_config.txt").unwrap_or_default();

    let first_line = content
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");

    let parts: Vec<&str> = first_line.split('=').collect();
    if parts.len() != 2 || parts[0].trim() != "listen" {
        return;
    }

    let addr_parts: Vec<&str> = parts[1].trim().split(':').collect();
    if addr_parts.len() != 2 {
        return;
    }

    let host = addr_parts[0];
    let port_str = addr_parts[1];

    if let Ok(port) = port_str.parse::<u16>() {
        println!("host = {}, port = {}", host, port);
    }
}
