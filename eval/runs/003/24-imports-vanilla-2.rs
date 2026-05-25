use std::collections::{HashMap, HashSet, BTreeMap, BTreeSet, VecDeque};
use std::io::{BufRead, BufReader, Read, Write, Cursor};
use std::path::{Path, PathBuf, Component};
use std::sync::{Arc, Mutex, RwLock};

fn main() {
    let index: Arc<Mutex<HashMap<PathBuf, HashSet<String>>>> = Arc::new(Mutex::new({
        let mut m = HashMap::new();
        let mut set1 = HashSet::new();
        set1.insert("alpha".to_string());
        set1.insert("beta".to_string());
        m.insert(PathBuf::from("/a/b.txt"), set1);

        let mut set2 = HashSet::new();
        set2.insert("gamma".to_string());
        m.insert(PathBuf::from("/c/d.txt"), set2);

        m
    }));

    let term_postings: Arc<RwLock<BTreeMap<String, BTreeSet<u32>>>> = Arc::new(RwLock::new({
        let mut m = BTreeMap::new();
        let mut set1 = BTreeSet::new();
        set1.insert(1);
        set1.insert(2);
        m.insert("alpha".to_string(), set1);

        let mut set2 = BTreeSet::new();
        set2.insert(2);
        set2.insert(3);
        m.insert("beta".to_string(), set2);

        m
    }));

    let mut queue: VecDeque<PathBuf> = VecDeque::new();
    queue.push_back(PathBuf::from("/a/b.txt"));
    queue.push_back(PathBuf::from("/c/d.txt"));

    for path in &queue {
        for component in path.components() {
            println!("{:?}", component);
        }
    }

    let cursor1 = Cursor::new(b"hello world".to_vec());
    let mut reader = BufReader::new(cursor1);
    let mut line = String::new();
    while reader.read_line(&mut line).unwrap() > 0 {
        println!("{}", line);
        line.clear();
    }

    let mut cursor2 = Cursor::new(Vec::<u8>::new());
    cursor2.write_all(b"goodbye").unwrap();

    let index_guard = index.lock().unwrap();
    println!("{:?}", *index_guard);

    let term_postings_guard = term_postings.read().unwrap();
    println!("{:?}", *term_postings_guard);
}
