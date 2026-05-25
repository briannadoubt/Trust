use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, VecDeque};

fn main() {
    let mut map: HashMap<&str, i32> = HashMap::new();
    map.insert("alpha", 1);
    map.insert("beta", 2);
    map.insert("gamma", 3);

    let mut set: HashSet<i32> = HashSet::new();
    set.insert(10);
    set.insert(20);
    set.insert(30);

    let mut btree_map: BTreeMap<&str, i32> = BTreeMap::new();
    btree_map.insert("one", 1);
    btree_map.insert("three", 3);
    btree_map.insert("two", 2);

    let mut btree_set: BTreeSet<i32> = BTreeSet::new();
    btree_set.insert(100);
    btree_set.insert(200);
    btree_set.insert(300);

    let mut deque: VecDeque<i32> = VecDeque::new();
    deque.push_back(7);
    deque.push_back(8);
    deque.push_back(9);

    let mut heap: BinaryHeap<i32> = BinaryHeap::new();
    heap.push(42);
    heap.push(17);
    heap.push(99);

    println!("{:?}", map);
    println!("{:?}", set);
    println!("{:?}", btree_map);
    println!("{:?}", btree_set);
    println!("{:?}", deque);
    println!("{:?}", heap);
}
