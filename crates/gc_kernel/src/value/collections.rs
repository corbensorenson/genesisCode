use std::collections::BTreeMap;

use gc_coreform::TermOrdKey;

use super::Value;
use crate::Shared;

#[derive(Clone, Debug, Default)]
pub struct ValueMap(pub(super) BTreeMap<TermOrdKey, Value>);

impl ValueMap {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    pub fn size(&self) -> usize {
        self.0.len()
    }

    pub fn get(&self, key: &TermOrdKey) -> Option<&Value> {
        self.0.get(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&TermOrdKey, &Value)> {
        self.0.iter()
    }

    pub fn insert_mut(&mut self, key: TermOrdKey, value: Value) {
        self.0.insert(key, value);
    }
}

impl FromIterator<(TermOrdKey, Value)> for ValueMap {
    fn from_iter<T: IntoIterator<Item = (TermOrdKey, Value)>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl<'a> IntoIterator for &'a ValueMap {
    type Item = (&'a TermOrdKey, &'a Value);
    type IntoIter = std::collections::btree_map::Iter<'a, TermOrdKey, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

#[derive(Clone, Debug)]
pub enum ValueVector {
    Flat(Vec<Value>),
}

impl ValueVector {
    pub fn new() -> Self {
        Self::Flat(Vec::new())
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Flat(xs) => xs.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, idx: usize) -> Option<&Value> {
        match self {
            Self::Flat(xs) => xs.get(idx),
        }
    }

    pub fn iter(&self) -> Box<dyn Iterator<Item = &Value> + '_> {
        match self {
            Self::Flat(xs) => Box::new(xs.iter()),
        }
    }

    pub fn push_shared(xs: &mut Shared<Self>, value: Value) {
        match Shared::make_mut(xs) {
            Self::Flat(vec) => vec.push(value),
        }
    }

    pub fn set_shared(xs: &mut Shared<Self>, idx: usize, value: Value) -> bool {
        if idx >= xs.len() {
            return false;
        }
        match Shared::make_mut(xs) {
            Self::Flat(vec) => {
                vec[idx] = value;
                true
            }
        }
    }
}

impl Default for ValueVector {
    fn default() -> Self {
        Self::new()
    }
}

impl FromIterator<Value> for ValueVector {
    fn from_iter<T: IntoIterator<Item = Value>>(iter: T) -> Self {
        Self::Flat(iter.into_iter().collect())
    }
}
