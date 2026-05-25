#![strict]

use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, VecDeque};

fn main() {
    let mut hash_map: HashMap<&str, i32> = HashMap::new();
    hash_map.insert("alpha", 1);
    hash_map.insert("beta", 2);
    hash_map.insert("gamma", 3);

    let mut hash_set: HashSet<&str> = HashSet::new();
    hash_set.insert("red");
    hash_set.insert("green");
    hash_set.insert("blue");

    let mut btree_map: BTreeMap<&str, i32> = BTreeMap::new();
    btree_map.insert("one", 1);
    btree_map.insert("two", 2);
    btree_map.insert("three", 3);

    let mut btree_set: BTreeSet<i32> = BTreeSet::new();
    btree_set.insert(10);
    btree_set.insert(20);
    btree_set.insert(30);

    let mut vec_deque: VecDeque<i32> = VecDeque::new();
    vec_deque.push_back(100);
    vec_deque.push_back(200);
    vec_deque.push_back(300);

    let mut binary_heap: BinaryHeap<i32> = BinaryHeap::new();
    binary_heap.push(42);
    binary_heap.push(17);
    binary_heap.push(99);

    println!("{:?}", hash_map);
    println!("{:?}", hash_set);
    println!("{:?}", btree_map);
    println!("{:?}", btree_set);
    println!("{:?}", vec_deque);
    println!("{:?}", binary_heap);
}
