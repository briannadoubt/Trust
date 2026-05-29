#![strict]

use std::collections::{HashMap, HashSet, BTreeMap, BTreeSet, VecDeque, BinaryHeap};

fn main() {
    // HashMap
    let mut hash_map = HashMap::new();
    hash_map.insert("apple", 1);
    hash_map.insert("banana", 2);
    hash_map.insert("cherry", 3);
    println!("{:?}", hash_map);

    // HashSet
    let mut hash_set = HashSet::new();
    hash_set.insert("red");
    hash_set.insert("green");
    hash_set.insert("blue");
    println!("{:?}", hash_set);

    // BTreeMap
    let mut btree_map = BTreeMap::new();
    btree_map.insert(3, "three");
    btree_map.insert(1, "one");
    btree_map.insert(2, "two");
    println!("{:?}", btree_map);

    // BTreeSet
    let mut btree_set = BTreeSet::new();
    btree_set.insert(30);
    btree_set.insert(10);
    btree_set.insert(20);
    println!("{:?}", btree_set);

    // VecDeque
    let mut vec_deque = VecDeque::new();
    vec_deque.push_back(100);
    vec_deque.push_front(50);
    vec_deque.push_back(200);
    println!("{:?}", vec_deque);

    // BinaryHeap
    let mut binary_heap = BinaryHeap::new();
    binary_heap.push(5);
    binary_heap.push(1);
    binary_heap.push(10);
    println!("{:?}", binary_heap);
}