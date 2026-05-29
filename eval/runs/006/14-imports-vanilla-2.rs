use std::collections::{HashMap, HashSet, BTreeMap, BTreeSet, VecDeque, BinaryHeap};

fn main() {
    let mut hash_map = HashMap::new();
    hash_map.insert("apple".to_string(), 1);
    hash_map.insert("banana".to_string(), 2);
    hash_map.insert("cherry".to_string(), 3);
    println!("{:?}", hash_map);

    let mut hash_set = HashSet::new();
    hash_set.insert("red".to_string());
    hash_set.insert("green".to_string());
    hash_set.insert("blue".to_string());
    println!("{:?}", hash_set);

    let mut btree_map = BTreeMap::new();
    btree_map.insert(10, "ten");
    btree_map.insert(20, "twenty");
    btree_map.insert(30, "thirty");
    println!("{:?}", btree_map);

    let mut btree_set = BTreeSet::new();
    btree_set.insert('a');
    btree_set.insert('b');
    btree_set.insert('c');
    println!("{:?}", btree_set);

    let mut vec_deque = VecDeque::new();
    vec_deque.push_back(1.1);
    vec_deque.push_back(2.2);
    vec_deque.push_back(3.3);
    println!("{:?}", vec_deque);

    let mut binary_heap = BinaryHeap::new();
    binary_heap.push(50);
    binary_heap.push(10);
    binary_heap.push(30);
    println!("{:?}", binary_heap);
}