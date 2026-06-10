//! REFERENCE SOLUTION — never shown to the agent. The fast-path guard is
//! scoped in its own block and dropped before the `.await`; the write lock is
//! re-acquired after the fetch completes. `cargo trust build` is green.

use std::collections::HashMap;
use std::sync::Mutex;

async fn fetch_remote(key: u32) -> String {
    std::future::ready(format!("remote-{key}")).await
}

async fn get_cached(cache: &Mutex<HashMap<u32, String>>, key: u32) -> String {
    {
        let guard = cache.lock().expect("cache mutex poisoned");
        if let Some(hit) = guard.get(&key) {
            return hit.clone();
        }
    }
    let fresh = fetch_remote(key).await;
    let mut guard = cache.lock().expect("cache mutex poisoned");
    guard.insert(key, fresh.clone());
    fresh
}

fn main() {
    let cache = Mutex::new(HashMap::new());
    let _pending = get_cached(cache: &cache, key: 7);
    println!("constructed cache lookup future");
}
