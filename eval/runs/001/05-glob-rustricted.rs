#![strict]

use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

fn main() {
    let mut hm: HashMap<u32, &str> = HashMap::new();
    hm.insert(1, "a");
    hm.insert(2, "b");
    println!("{:?}", hm);

    let mut hs: HashSet<u32> = HashSet::new();
    hs.insert(1);
    hs.insert(2);
    println!("{:?}", hs);

    let mut bm: BTreeMap<u32, &str> = BTreeMap::new();
    bm.insert(1, "a");
    bm.insert(2, "b");
    println!("{:?}", bm);

    let mut bs: BTreeSet<u32> = BTreeSet::new();
    bs.insert(1);
    bs.insert(2);
    println!("{:?}", bs);
}
