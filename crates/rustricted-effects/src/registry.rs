//! Effect identifiers + sets.

use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct Effect(pub String);

impl Effect {
    pub fn new(name: impl Into<String>) -> Self {
        Effect(name.into())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EffectSet(pub BTreeSet<Effect>);

impl EffectSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_names<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        EffectSet(names.into_iter().map(|n| Effect::new(n.into())).collect())
    }

    pub fn is_subset_of(&self, other: &EffectSet) -> bool {
        self.0.is_subset(&other.0)
    }

    pub fn union_with(&mut self, other: &EffectSet) {
        for e in &other.0 {
            self.0.insert(e.clone());
        }
    }
}

/// Per-crate table mapping function path → declared effect set.
#[derive(Debug, Default)]
pub struct EffectTable {
    pub fns: std::collections::HashMap<String, EffectSet>,
}

impl EffectTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: impl Into<String>, effects: EffectSet) {
        self.fns.insert(path.into(), effects);
    }

    pub fn get(&self, path: &str) -> Option<&EffectSet> {
        self.fns.get(path)
    }
}
