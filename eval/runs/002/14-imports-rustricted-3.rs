#![strict]

use std::collections::{HashMap, HashSet, BTreeMap, BTreeSet, VecDeque, BinaryHeap};

fn main() {
    let mut hash_map = HashMap::new();
    hash_map.insert("apple", 5);
    hash_map.insert("banana", 3);
    hash_map.insert("cherry", 8);
    println!("{:?}", hash_map);

    let mut hash_set = HashSet::new();
    hash_set.insert(42);
    hash_set.insert(17);
    hash_set.insert(99);
    println!("{:?}", hash_set);

    let mut btree_map = BTreeMap::new();
    btree_map.insert(1, "first");
    btree_map.insert(2, "second");
    btree_map.insert(3, "third");
    println!("{:?}", btree_map);

    let mut btree_set = BTreeSet::new();
    btree_set.insert(10);
    btree_set.insert(20);
    btree_set.insert(15);
    println!("{:?}", btree_set);

    let mut vec_deque = VecDeque::new();
    vec_deque.push_back("one");
    vec_deque.push_back("two");
    vec_deque.push_back("three");
    println!("{:?}", vec_deque);

    let mut binary_heap = BinaryHeap::new();
    binary_heap.push(100);
    binary_heap.push(50);
    binary_heap.push(75);
    println!("{:?}", binary_heap);
}
