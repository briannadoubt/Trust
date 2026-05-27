//! Named-argument shims over `std`.
//!
//! Each wrapper exists so Rustricted callers can write call sites with
//! `name: value` syntax: `fs::read_to_string(path: p)` instead of the
//! positional `std::fs::read_to_string(p)`. The wrappers carry no logic;
//! they exist purely to make parameter names part of the signature for
//! Rustricted's named-args lowering pass.
//!
//! Intentionally not `#![strict]`-marked: this crate is the source of truth
//! for the `STD_SIGNATURES` build-time index in `rustricted-lower/build.rs`,
//! which parses this file with `syn`. Strict-mode syntax (named args, pipe)
//! is not parseable by stock `syn`, so strict-marking here would empty the
//! signature index and break cross-crate named-arg lowering everywhere.
//! See dogfooding case study + RT-46.

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
    use std::collections::{BTreeMap, HashMap, HashSet};
    use std::hash::Hash;

    /// Insert `value` into `map` under `key`, returning the previous value.
    pub fn insert<K: Eq + Hash, V>(map: &mut HashMap<K, V>, key: K, value: V) -> Option<V> {
        map.insert(key, value)
    }

    /// Construct an empty [`HashMap`].
    pub fn hashmap_new<K, V>() -> HashMap<K, V> {
        HashMap::new()
    }

    /// Construct a [`HashMap`] pre-allocated for at least `capacity` entries.
    pub fn hashmap_with_capacity<K, V>(capacity: usize) -> HashMap<K, V> {
        HashMap::with_capacity(capacity)
    }

    /// Insert `value` into `map` under `key`, returning the previous value.
    ///
    /// Alias of [`insert`] with the more explicit `hashmap_` prefix used by
    /// the other constructors in this module.
    pub fn hashmap_insert<K: Eq + Hash, V>(map: &mut HashMap<K, V>, key: K, value: V) -> Option<V> {
        map.insert(key, value)
    }

    /// Construct an empty [`BTreeMap`].
    pub fn btreemap_new<K, V>() -> BTreeMap<K, V> {
        BTreeMap::new()
    }

    /// Construct an empty [`HashSet`].
    pub fn hashset_new<T>() -> HashSet<T> {
        HashSet::new()
    }
}

pub mod net {
    use std::io;
    use std::net::{TcpListener, TcpStream, UdpSocket};

    /// Bind a TCP listener to `addr`.
    pub fn tcp_listener_bind(addr: &str) -> io::Result<TcpListener> {
        TcpListener::bind(addr)
    }

    /// Open a TCP connection to `addr`.
    pub fn tcp_connect(addr: &str) -> io::Result<TcpStream> {
        TcpStream::connect(addr)
    }

    /// Bind a UDP socket to `addr`.
    pub fn udp_socket_bind(addr: &str) -> io::Result<UdpSocket> {
        UdpSocket::bind(addr)
    }
}

pub mod sync {
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::sync::{Arc, Mutex, RwLock};

    /// Wrap `value` in a [`Mutex`].
    pub fn mutex_new<T>(value: T) -> Mutex<T> {
        Mutex::new(value)
    }

    /// Wrap `value` in an [`RwLock`].
    pub fn rwlock_new<T>(value: T) -> RwLock<T> {
        RwLock::new(value)
    }

    /// Wrap `value` in an [`Arc`].
    pub fn arc_new<T>(value: T) -> Arc<T> {
        Arc::new(value)
    }

    /// Create a new asynchronous channel, returning `(sender, receiver)`.
    ///
    /// Takes no arguments, so named-arg lowering is a no-op here; the shim
    /// exists for module-level discoverability alongside the other helpers.
    pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
        mpsc::channel()
    }
}

pub mod process {
    use std::process::Command;

    /// Construct a [`Command`] that will spawn `program`.
    pub fn command(program: &str) -> Command {
        Command::new(program)
    }

    /// Append a single positional `arg` to `cmd`, returning the same `cmd`
    /// for chaining.
    ///
    /// Note: `Command::arg` is defined as `&mut self -> &mut Command`, which
    /// the named-arg lowering pass treats as a method shape. This free-fn
    /// form re-exposes it with explicit parameter names.
    pub fn command_arg<'a>(cmd: &'a mut Command, arg: &str) -> &'a mut Command {
        cmd.arg(arg)
    }

    /// Set an environment variable on `cmd`, returning the same `cmd` for
    /// chaining.
    pub fn command_env<'a>(cmd: &'a mut Command, key: &str, value: &str) -> &'a mut Command {
        cmd.env(key, value)
    }

    /// Terminate the current process with exit status `code`.
    pub fn exit(code: i32) -> ! {
        std::process::exit(code)
    }
}

pub mod string {
    /// Construct an empty [`String`].
    pub fn string_new() -> String {
        String::new()
    }

    /// Construct a [`String`] with capacity for at least `capacity` bytes.
    pub fn string_with_capacity(capacity: usize) -> String {
        String::with_capacity(capacity)
    }
}

pub mod vec {
    /// Construct an empty [`Vec`].
    pub fn vec_new<T>() -> Vec<T> {
        Vec::new()
    }

    /// Construct a [`Vec`] pre-allocated for at least `capacity` items.
    pub fn vec_with_capacity<T>(capacity: usize) -> Vec<T> {
        Vec::with_capacity(capacity)
    }

    /// Push `value` onto the end of `v`.
    pub fn vec_push<T>(v: &mut Vec<T>, value: T) {
        v.push(value);
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

    #[test]
    fn collections_shims_construct_empty_containers() {
        let mut m: std::collections::HashMap<&str, i32> = collections::hashmap_new();
        assert!(collections::hashmap_insert(&mut m, "a", 1).is_none());
        assert_eq!(m.get("a"), Some(&1));

        let m2: std::collections::HashMap<&str, i32> = collections::hashmap_with_capacity(8);
        assert!(m2.capacity() >= 8);

        let bm: std::collections::BTreeMap<&str, i32> = collections::btreemap_new();
        assert!(bm.is_empty());

        let hs: std::collections::HashSet<i32> = collections::hashset_new();
        assert!(hs.is_empty());
    }

    #[test]
    fn net_shims_bind_and_connect() {
        let listener = net::tcp_listener_bind("127.0.0.1:0").expect("bind tcp");
        let addr = listener.local_addr().expect("local addr");
        let _client = net::tcp_connect(&addr.to_string()).expect("connect tcp");
        let udp = net::udp_socket_bind("127.0.0.1:0").expect("bind udp");
        assert!(udp.local_addr().is_ok());
    }

    #[test]
    fn sync_shims_wrap_values() {
        let m = sync::mutex_new(7);
        assert_eq!(*m.lock().unwrap(), 7);
        let rw = sync::rwlock_new(9);
        assert_eq!(*rw.read().unwrap(), 9);
        let a = sync::arc_new(42);
        assert_eq!(*a, 42);
        let (tx, rx) = sync::channel::<i32>();
        tx.send(3).unwrap();
        assert_eq!(rx.recv().unwrap(), 3);
    }

    #[test]
    fn process_command_shims_chain() {
        let mut cmd = process::command("echo");
        process::command_arg(&mut cmd, "hello");
        process::command_env(&mut cmd, "RUSTRICTED_TEST", "1");
        let prog = cmd.get_program().to_owned();
        assert_eq!(prog, "echo");
    }

    #[test]
    fn string_and_vec_shims_construct() {
        let s = string::string_new();
        assert!(s.is_empty());
        let s2 = string::string_with_capacity(16);
        assert!(s2.capacity() >= 16);

        let v: Vec<i32> = vec::vec_new();
        assert!(v.is_empty());
        let mut v2: Vec<i32> = vec::vec_with_capacity(4);
        assert!(v2.capacity() >= 4);
        vec::vec_push(&mut v2, 1);
        vec::vec_push(&mut v2, 2);
        assert_eq!(v2, vec![1, 2]);
    }
}
