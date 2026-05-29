use std::collections::{HashMap, HashSet, BTreeMap, BTreeSet, VecDeque, BinaryHeap};

fn main() {
    let mut hashmap = HashMap::new();
    hashmap.insert("one", 1);
    hashmap.insert("two", 2);
    hashmap.insert("three", 3);
    println!("{:?}", hashmap);

    let mut hashset = HashSet::new();
    hashset.insert("apple");
    hashset.insert("banana");
    hashset.insert("cherry");
    println!("{:?}", hashset);

    let mut btreemap = BTreeMap::new();
    btreemap.insert(1, "a");
    btreemap.insert(2, "b");
    btreemap.insert(3, "c");
    println!("{:?}", btreemap);

    let mut btreeset = BTreeSet::new();
    btreeset.insert(10);
    btreeset.insert(20);
    btreeset.insert(30);
    println!("{:?}", btreeset);

    let mut vecdeque = VecDeque::new();
    vecdeque.push_back(100);
    vecdeque.push_back(200);
    vecdeque.push_back(300);
    println!("{:?}", vecdeque);

    let mut binaryheap = BinaryHeap::new();
    binaryheap.push(5);
    binaryheap.push(15);
    binaryheap.push(25);
    println!("{:?}", binaryheap);
}