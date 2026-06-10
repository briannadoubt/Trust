//! REFERENCE SOLUTION — never shown to the agent. The closure now carries
//! the source `ParseIntError` (`.map_err(|err| …)` per the R0018 `instead:`
//! guidance), so `cargo trust build` is green.

use std::fmt;

#[derive(Debug)]
enum ConfigError {
    BadPort {
        raw: String,
        source: std::num::ParseIntError,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::BadPort { raw, source } => {
                write!(f, "bad port value {raw:?}: {source}")
            }
        }
    }
}

fn parse_port(raw: &str) -> Result<u16, ConfigError> {
    raw.trim().parse::<u16>().map_err(|err| ConfigError::BadPort {
        raw: raw.to_string(),
        source: err,
    })
}

fn main() {
    match parse_port("8080") {
        Ok(port) => println!("port = {port}"),
        Err(err) => eprintln!("error: {err}"),
    }
}
