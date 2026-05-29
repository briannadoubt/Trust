#![strict]

use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::VecDeque;
use std::collections::BinaryHeap;

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
    vec_deque.push_back('a');
    vec_deque.push_back('b');
    vec_deque.push_back('c');
    println!("{:?}", vec_deque);

    // BinaryHeap
    let mut binary_heap = BinaryHeap::new();
    binary_heap.push(5);
    binary_heap.push(1);
    binary_heap.push(10);
    println!("{:?}", binary_heap);
}