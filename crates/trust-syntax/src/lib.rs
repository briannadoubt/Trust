//! Parsing and re-emission of Trust source.
//!
//! Phase 0 is an identity round-trip via `syn` + `prettyplease`. Later phases
//! extend the grammar with named arguments, the pipe operator, and the
//! `effect` keyword, and lower those forms back to plain Rust here.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("parse error: {0}")]
    Parse(#[from] syn::Error),
}

/// Parse Rust source and emit it back, formatted via `prettyplease`.
pub fn roundtrip(source: &str) -> Result<String, Error> {
    let file: syn::File = syn::parse_str(source)?;
    Ok(prettyplease::unparse(&file))
}

#[cfg(test)]
mod tests {
    use super::{roundtrip, Error};

    #[test]
    fn empty_file_roundtrips() {
        let out = roundtrip("").expect("empty file should parse");
        assert_eq!(out, "");
    }

    #[test]
    fn hello_world_survives_roundtrip() {
        let src = "fn main() { println!(\"hi\"); }";
        let out = roundtrip(src).expect("hello world should parse");
        assert!(out.contains("fn main"));
        assert!(out.contains("println"));
        assert!(out.contains("\"hi\""));
    }

    #[test]
    fn inner_attribute_survives_roundtrip() {
        let src = "#![allow(unused)]\nfn main() {}";
        let out = roundtrip(src).expect("inner attribute should parse");
        assert!(out.contains("#![allow(unused)]"));
    }

    #[test]
    fn parse_error_surfaces() {
        let err = roundtrip("fn (((").expect_err("garbage should not parse");
        match err {
            Error::Parse(_) => {}
        }
    }
}
