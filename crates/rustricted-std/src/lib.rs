//! Named-argument shims over `std`.
//!
//! Each wrapper exists so Rustricted callers can write call sites with
//! `name: value` syntax: `fs::read_to_string(path: p)` instead of the
//! positional `std::fs::read_to_string(p)`. The wrappers carry no logic;
//! they exist purely to make parameter names part of the signature for
//! Rustricted's named-args lowering pass.

pub mod fs {
    use std::io;
    use std::path::Path;

    /// Read the entire contents of a file into a `String`.
    pub fn read_to_string(path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    /// Write `contents` to a file at `path`, creating it if missing and
    /// truncating it if it already exists.
    pub fn write_text(path: &Path, contents: &str) -> io::Result<()> {
        std::fs::write(path, contents)
    }

    /// Write a byte buffer to a file.
    pub fn write_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
        std::fs::write(path, bytes)
    }

    /// Create a directory and any missing parents.
    pub fn create_dir_all(path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    /// Remove a single file.
    pub fn remove_file(path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    /// Copy `from` to `to`. Returns the number of bytes copied.
    pub fn copy(from: &Path, to: &Path) -> io::Result<u64> {
        std::fs::copy(from, to)
    }

    /// Rename / move `from` to `to`.
    pub fn rename(from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }
}

pub mod time {
    use std::time::Duration;

    /// Build a [`Duration`] from named `secs` and `nanos`.
    pub fn duration(secs: u64, nanos: u32) -> Duration {
        Duration::new(secs, nanos)
    }

    /// Build a [`Duration`] from a whole number of milliseconds.
    pub fn millis(value: u64) -> Duration {
        Duration::from_millis(value)
    }
}

pub mod env {
    use std::ffi::OsString;

    /// Set the value of an environment variable.
    pub fn set_var(name: &str, value: &str) {
        // safety: std::env::set_var is currently safe in 1.95 stable;
        // this shim future-proofs callers if that changes.
        // reason: documenting why this wraps a one-liner
        unsafe {
            std::env::set_var(name, value);
        }
    }

    /// Get an environment variable's value, if set.
    pub fn var(name: &str) -> Option<OsString> {
        std::env::var_os(name)
    }
}

pub mod thread {
    use std::time::Duration;

    /// Sleep the current thread for `duration`.
    pub fn sleep(duration: Duration) {
        std::thread::sleep(duration);
    }
}

pub mod collections {
    use std::collections::HashMap;
    use std::hash::Hash;

    /// Insert `value` into `map` under `key`, returning the previous value.
    pub fn insert<K: Eq + Hash, V>(map: &mut HashMap<K, V>, key: K, value: V) -> Option<V> {
        map.insert(key, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // This crate is compiled by plain `rustc`, so tests use positional
    // call syntax. The named-arg form (`time::duration(secs: 1, nanos: 500)`)
    // is exercised by the examples under `examples/06-stdlib/` which go
    // through the `rustricted` lowering pipeline.
    #[test]
    fn duration_helper_round_trips() {
        let d = time::duration(1, 500);
        assert_eq!(d, Duration::new(1, 500));
    }

    #[test]
    fn millis_helper_round_trips() {
        let d = time::millis(250);
        assert_eq!(d, Duration::from_millis(250));
    }
}
