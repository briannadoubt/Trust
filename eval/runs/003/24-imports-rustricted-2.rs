#![strict]

use std::collections::{HashMap, HashSet, BTreeMap, BTreeSet, VecDeque};
use std::io::{BufRead, BufReader, Read, Write, Cursor};
use std::path::{Path, PathBuf, Component};
use std::sync::{Arc, Mutex, RwLock};

fn main() {
    // 1. Build Arc<Mutex<HashMap<PathBuf, HashSet<String>>>>
    let index = Arc::new(Mutex::new(HashMap::new()));
    {
        let mut idx = index.lock().unwrap();
        let mut set1 = HashSet::new();
        set1.insert("alpha".to_string());
        set1.insert("beta".to_string());
        idx.insert(PathBuf::from("/a/b.txt"), set1);

        let mut set2 = HashSet::new();
        set2.insert("gamma".to_string());
        idx.insert(PathBuf::from("/c/d.txt"), set2);
    }

    // 2. Build Arc<RwLock<BTreeMap<String, BTreeSet<u32>>>>
    let term_postings = Arc::new(RwLock::new(BTreeMap::new()));
    {
        let mut tp = term_postings.write().unwrap();
        let mut set_alpha = BTreeSet::new();
        set_alpha.insert(1);
        set_alpha.insert(2);
        tp.insert("alpha".to_string(), set_alpha);

        let mut set_beta = BTreeSet::new();
        set_beta.insert(2);
        set_beta.insert(3);
        tp.insert("beta".to_string(), set_beta);
    }

    // 3. Build VecDeque and walk Components
    let mut queue: VecDeque<PathBuf> = VecDeque::new();
    queue.push_back(PathBuf::from("/a/b.txt"));
    queue.push_back(PathBuf::from("/c/d.txt"));

    while let Some(path) = queue.pop_front() {
        for component in path.components() {
            println!("{:?}", component);
        }
    }

    // 4. Cursor with BufReader, Read, Write
    let data = b"hello world".to_vec();
    let cursor = Cursor::new(data);
    let mut reader = BufReader::new(cursor);
    let mut line = String::new();
    while reader.read_line(&mut line).unwrap() > 0 {
        println!("{}", line.trim());
        line.clear();
    }

    let empty_vec: Vec<u8> = Vec::new();
    let mut cursor2 = Cursor::new(empty_vec);
    cursor2.write_all(b"goodbye").unwrap();

    // 5. Print final index and term_postings
    let final_index = index.lock().unwrap();
    println!("{:?}", *final_index);

    let final_tp = term_postings.read().unwrap();
    println!("{:?}", *final_tp);
}
