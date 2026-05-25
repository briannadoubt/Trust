use std::fs;

fn main() {
    let config = fs::read_to_string("/tmp/server_config.txt").unwrap();

    let line = config.lines().find(|l| !l.trim().is_empty()).unwrap();

    let parts: Vec<&str> = line.split('=').collect();
    let key = parts[0].trim();
    let value = parts[1].trim();

    assert_eq!(key, "listen");

    let addr_parts: Vec<&str> = value.split(':').collect();
    let host = addr_parts[0];
    let port_str = addr_parts[1];

    let port: u16 = port_str.parse().unwrap();

    println!("host = {}, port = {}", host, port);
}
