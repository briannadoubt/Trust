//! Smoke tests for the std-shim wrappers.
//!
//! Intentionally NOT `#![strict]`-marked even though `lib.rs` is. Several
//! shims in this crate are generic (e.g. `hashmap_insert<K, V>(...)`,
//! `vec_push<T>(...)`), and the per-file `CalleeRegistry` token scan in
//! `rustricted-lower` currently miscounts arity for generic fns whose
//! signatures contain joint-spacing `>` characters (the `>>` in `Vec<T>>`
//! decrements `angle_depth` incorrectly, leaving a comma trapped inside
//! a phantom angle group). The bug only bites when *both* (a) the lib is
//! strict and (b) the same file calls its own generic shims positionally
//! — exactly the test situation. Hoisting the tests to a non-strict
//! sibling file dodges the issue without weakening coverage. Filed as a
//! follow-up; see RT-44 case-study notes in `dogfooding.md`.

use super::*;
use std::time::Duration;

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
