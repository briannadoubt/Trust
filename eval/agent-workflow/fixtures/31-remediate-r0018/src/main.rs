//! Eval fixture: the `.map_err(|_| …)` below discards the source
//! `ParseIntError`, so `cargo trustc build` fails with exactly R0018.

use std::fmt;

#[derive(Debug)]
enum ConfigError {
    BadPort { raw: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::BadPort { raw } => write!(f, "bad port value {raw:?}"),
        }
    }
}

fn parse_port(raw: &str) -> Result<u16, ConfigError> {
    raw.trim()
        .parse::<u16>()
        .map_err(|_| ConfigError::BadPort { raw: raw.to_string() })
}

fn main() {
    match parse_port("8080") {
        Ok(port) => println!("port = {port}"),
        Err(err) => eprintln!("error: {err}"),
    }
}
