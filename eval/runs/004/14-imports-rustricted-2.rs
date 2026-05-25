#![strict]

use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, VecDeque};

fn main() {
    let mut hash_map: HashMap<&str, i32> = HashMap::new();
    hash_map.insert("alpha", 1);
    hash_map.insert("beta", 2);
    hash_map.insert("gamma", 3);

    let mut hash_set: HashSet<i32> = HashSet::new();
    hash_set.insert(10);
    hash_set.insert(20);
    hash_set.insert(30);

    let mut btree_map: BTreeMap<&str, i32> = BTreeMap::new();
    btree_map.insert("one", 1);
    btree_map.insert("three", 3);
    btree_map.insert("two", 2);

    let mut btree_set: BTreeSet<i32> = BTreeSet::new();
    btree_set.insert(100);
    btree_set.insert(200);
    btree_set.insert(300);

    let mut vec_deque: VecDeque<i32> = VecDeque::new();
    vec_deque.push_back(7);
    vec_deque.push_back(8);
    vec_deque.push_back(9);

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
