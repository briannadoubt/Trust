use std::collections::{HashMap, HashSet, BTreeMap, BTreeSet, VecDeque, BinaryHeap};

fn main() {
    let mut hash_map = HashMap::new();
    hash_map.insert("a", 1);
    hash_map.insert("b", 2);
    hash_map.insert("c", 3);
    println!("{:?}", hash_map);

    let mut hash_set = HashSet::new();
    hash_set.insert(10);
    hash_set.insert(20);
    hash_set.insert(30);
    println!("{:?}", hash_set);

    let mut btree_map = BTreeMap::new();
    btree_map.insert("x", 100);
    btree_map.insert("y", 200);
    btree_map.insert("z", 300);
    println!("{:?}", btree_map);

    let mut btree_set = BTreeSet::new();
    btree_set.insert(5);
    btree_set.insert(15);
    btree_set.insert(25);
    println!("{:?}", btree_set);

    let mut vec_deque = VecDeque::new();
    vec_deque.push_back("first");
    vec_deque.push_back("second");
    vec_deque.push_back("third");
    println!("{:?}", vec_deque);

    let mut binary_heap = BinaryHeap::new();
    binary_heap.push(7);
    binary_heap.push(3);
    binary_heap.push(11);
    println!("{:?}", binary_heap);
}
