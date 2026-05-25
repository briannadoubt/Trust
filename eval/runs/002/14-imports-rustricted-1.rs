#![strict]

use std::collections::{HashMap, HashSet, BTreeMap, BTreeSet, VecDeque, BinaryHeap};

fn main() {
    let mut hash_map = HashMap::new();
    hash_map.insert("apple", 5);
    hash_map.insert("banana", 3);
    hash_map.insert("cherry", 8);
    println!("{:?}", hash_map);

    let mut hash_set = HashSet::new();
    hash_set.insert("red");
    hash_set.insert("blue");
    hash_set.insert("green");
    println!("{:?}", hash_set);

    let mut btree_map = BTreeMap::new();
    btree_map.insert(1, "one");
    btree_map.insert(2, "two");
    btree_map.insert(3, "three");
    println!("{:?}", btree_map);

    let mut btree_set = BTreeSet::new();
    btree_set.insert(10);
    btree_set.insert(20);
    btree_set.insert(30);
    println!("{:?}", btree_set);

    let mut vec_deque = VecDeque::new();
    vec_deque.push_back('a');
    vec_deque.push_back('b');
    vec_deque.push_back('c');
    println!("{:?}", vec_deque);

    let mut binary_heap = BinaryHeap::new();
    binary_heap.push(42);
    binary_heap.push(17);
    binary_heap.push(93);
    println!("{:?}", binary_heap);
}
