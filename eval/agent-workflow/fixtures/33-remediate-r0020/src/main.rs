//! Eval fixture: the `std::sync::Mutex` guard is still alive across the
//! `.await` in `get_cached` — a deadlock under a single-threaded executor.
//! `cargo trustc build` fails with exactly R0020.
//!
//! std-only on purpose: `main` just constructs the future so the fixture
//! builds without pulling in an async runtime.

use std::collections::HashMap;
use std::sync::Mutex;

async fn fetch_remote(key: u32) -> String {
    std::future::ready(format!("remote-{key}")).await
}

async fn get_cached(cache: &Mutex<HashMap<u32, String>>, key: u32) -> String {
    let mut guard = cache.lock().expect("cache mutex poisoned");
    if let Some(hit) = guard.get(&key) {
        return hit.clone();
    }
    let fresh = fetch_remote(key).await;
    guard.insert(key, fresh.clone());
    fresh
}

fn main() {
    let cache = Mutex::new(HashMap::new());
    let _pending = get_cached(cache: &cache, key: 7);
    println!("constructed cache lookup future");
}
