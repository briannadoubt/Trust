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
    btreemap.insert("x", 24);
    btreemap.insert("y", 25);
    btreemap.insert("z", 26);
    println!("{:?}", btreemap);

    let mut btreeset = BTreeSet::new();
    btreeset.insert(10);
    btreeset.insert(20);
    btreeset.insert(30);
    println!("{:?}", btreeset);

    let mut vecdeque = VecDeque::new();
    vecdeque.push_back("front");
    vecdeque.push_back("middle");
    vecdeque.push_back("back");
    println!("{:?}", vecdeque);

    let mut binaryheap = BinaryHeap::new();
    binaryheap.push(100);
    binaryheap.push(50);
    binaryheap.push(75);
    println!("{:?}", binaryheap);
}