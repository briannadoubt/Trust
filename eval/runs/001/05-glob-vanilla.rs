use std::collections::{HashMap, HashSet, BTreeMap, BTreeSet};

fn main() {
    let mut hashmap: HashMap<u32, &str> = HashMap::new();
    hashmap.insert(1, "a");
    hashmap.insert(2, "b");
    println!("{:?}", hashmap);

    let mut hashset: HashSet<u32> = HashSet::new();
    hashset.insert(1);
    hashset.insert(2);
    println!("{:?}", hashset);

    let mut btreemap: BTreeMap<u32, &str> = BTreeMap::new();
    btreemap.insert(1, "a");
    btreemap.insert(2, "b");
    println!("{:?}", btreemap);

    let mut btreeset: BTreeSet<u32> = BTreeSet::new();
    btreeset.insert(1);
    btreeset.insert(2);
    println!("{:?}", btreeset);
}
