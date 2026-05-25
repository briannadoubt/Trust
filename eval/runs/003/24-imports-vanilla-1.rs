use std::collections::{HashMap, HashSet, BTreeMap, BTreeSet, VecDeque};
use std::io::{BufRead, BufReader, Read, Write, Cursor};
use std::path::{Path, PathBuf, Component};
use std::sync::{Arc, Mutex, RwLock};

fn main() {
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

    let term_postings = Arc::new(RwLock::new(BTreeMap::new()));
    {
        let mut tp = term_postings.write().unwrap();
        let mut postings1 = BTreeSet::new();
        postings1.insert(1);
        postings1.insert(2);
        tp.insert("alpha".to_string(), postings1);

        let mut postings2 = BTreeSet::new();
        postings2.insert(2);
        postings2.insert(3);
        tp.insert("beta".to_string(), postings2);
    }

    let mut queue = VecDeque::new();
    queue.push_back(PathBuf::from("/a/b.txt"));
    queue.push_back(PathBuf::from("/c/d.txt"));

    while let Some(path) = queue.pop_front() {
        for component in path.components() {
            println!("{:?}", component);
        }
    }

    let cursor = Cursor::new(b"hello world".to_vec());
    let mut reader = BufReader::new(cursor);
    let mut line = String::new();
    while reader.read_line(&mut line).unwrap() > 0 {
        println!("{}", line.trim());
        line.clear();
    }

    let mut cursor2 = Cursor::new(Vec::<u8>::new());
    cursor2.write_all(b"goodbye").unwrap();

    let idx = index.lock().unwrap();
    println!("index: {:?}", *idx);

    let tp = term_postings.read().unwrap();
    println!("term_postings: {:?}", *tp);
}
