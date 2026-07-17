use std::collections::BTreeMap;
#[cfg(test)]
use std::collections::BTreeSet;

use gc_coreform::TermOrdKey;
use rust_cc::{Context, Finalize, Trace};

use crate::{Shared, Value};

const PAGE_CAPACITY: usize = 32;
const PERSISTENT_FREEZE_THRESHOLD: usize = 4_096;
type Entry = (TermOrdKey, Value);
type Link = Option<Shared<MapNode>>;

#[derive(Clone, Debug)]
pub struct ValueMap {
    storage: MapStorage,
}

#[derive(Clone, Debug)]
enum MapStorage {
    Flat(BTreeMap<TermOrdKey, Value>),
    Persistent(PersistentMap),
}

#[derive(Clone, Debug, Default)]
struct PersistentMap {
    root: Link,
    len: usize,
}

#[derive(Clone, Debug)]
enum MapNode {
    Leaf(Vec<Entry>),
    Branch {
        children: Vec<Shared<MapNode>>,
        max_keys: Vec<TermOrdKey>,
        height: u16,
    },
}

impl Default for ValueMap {
    fn default() -> Self {
        Self {
            storage: MapStorage::Flat(BTreeMap::new()),
        }
    }
}

impl ValueMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn size(&self) -> usize {
        match &self.storage {
            MapStorage::Flat(entries) => entries.len(),
            MapStorage::Persistent(map) => map.len,
        }
    }

    pub fn get(&self, key: &TermOrdKey) -> Option<&Value> {
        match &self.storage {
            MapStorage::Flat(entries) => entries.get(key),
            MapStorage::Persistent(map) => map.get(key),
        }
    }

    pub fn iter(&self) -> Box<dyn Iterator<Item = (&TermOrdKey, &Value)> + '_> {
        match &self.storage {
            MapStorage::Flat(entries) => Box::new(entries.iter()),
            MapStorage::Persistent(map) => Box::new(MapIter::new(map.root.as_deref())),
        }
    }

    pub fn insert_mut(&mut self, key: TermOrdKey, value: Value) {
        match &mut self.storage {
            MapStorage::Flat(entries) => {
                entries.insert(key, value);
            }
            MapStorage::Persistent(map) => map.insert_mut(key, value),
        }
    }

    pub fn insert_shared(map: &mut Shared<Self>, key: TermOrdKey, value: Value) {
        if map.strong_count() > 1
            && let MapStorage::Flat(entries) = &map.storage
            && entries.len() >= PERSISTENT_FREEZE_THRESHOLD
        {
            let mut persistent = PersistentMap::from_sorted(
                entries
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone())),
            );
            persistent.insert_mut(key, value);
            *map = Shared::new(Self {
                storage: MapStorage::Persistent(persistent),
            });
            return;
        }
        Shared::make_mut(map).insert_mut(key, value);
    }

    pub(crate) fn trace_owners(&self, ctx: &mut Context<'_>) {
        match &self.storage {
            MapStorage::Flat(entries) => {
                for value in entries.values() {
                    value.trace(ctx);
                }
            }
            MapStorage::Persistent(map) => map.root.trace(ctx),
        }
    }

    #[cfg(test)]
    pub(crate) fn collect_node_identities(&self, identities: &mut BTreeSet<usize>) {
        let MapStorage::Persistent(map) = &self.storage else {
            return;
        };
        let mut stack = map.root.iter().collect::<Vec<_>>();
        while let Some(node) = stack.pop() {
            let identity = node.identity_ptr() as usize;
            if !identities.insert(identity) {
                continue;
            }
            if let MapNode::Branch { children, .. } = node.as_ref() {
                stack.extend(children);
            }
        }
    }
}

impl FromIterator<Entry> for ValueMap {
    fn from_iter<T: IntoIterator<Item = Entry>>(iter: T) -> Self {
        Self {
            storage: MapStorage::Flat(iter.into_iter().collect()),
        }
    }
}

impl<'a> IntoIterator for &'a ValueMap {
    type Item = (&'a TermOrdKey, &'a Value);
    type IntoIter = Box<dyn Iterator<Item = Self::Item> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl PersistentMap {
    fn from_sorted(iter: impl IntoIterator<Item = Entry>) -> Self {
        let entries = iter.into_iter().collect::<Vec<_>>();
        let len = entries.len();
        let mut nodes = entries
            .chunks(PAGE_CAPACITY)
            .map(|chunk| leaf(chunk.to_vec()))
            .collect::<Vec<_>>();
        while nodes.len() > 1 {
            nodes = nodes
                .chunks(PAGE_CAPACITY)
                .map(|chunk| branch(chunk.to_vec()))
                .collect();
        }
        Self {
            root: nodes.pop(),
            len,
        }
    }

    fn get(&self, key: &TermOrdKey) -> Option<&Value> {
        let mut cursor = self.root.as_deref()?;
        loop {
            match cursor {
                MapNode::Leaf(entries) => {
                    return entries
                        .binary_search_by(|(candidate, _)| candidate.cmp(key))
                        .ok()
                        .and_then(|index| entries.get(index))
                        .map(|(_, value)| value);
                }
                MapNode::Branch {
                    children, max_keys, ..
                } => {
                    let index = child_index(max_keys, key, children.len())?;
                    cursor = children.get(index)?;
                }
            }
        }
    }

    fn insert_mut(&mut self, key: TermOrdKey, value: Value) {
        let Some(root) = self.root.as_mut() else {
            self.root = Some(leaf(vec![(key, value)]));
            self.len = 1;
            return;
        };
        let (inserted, overflow) = insert(root, key, value);
        if let Some(right) = overflow
            && let Some(left) = self.root.take()
        {
            self.root = Some(branch(vec![left, right]));
        }
        self.len = self.len.saturating_add(usize::from(inserted));
    }
}

struct MapIter<'a> {
    stack: Vec<&'a MapNode>,
    leaf: Option<std::slice::Iter<'a, Entry>>,
}

impl<'a> MapIter<'a> {
    fn new(root: Option<&'a MapNode>) -> Self {
        let mut iter = Self {
            stack: Vec::new(),
            leaf: None,
        };
        if let Some(root) = root {
            iter.stack.push(root);
        }
        iter
    }
}

impl<'a> Iterator for MapIter<'a> {
    type Item = (&'a TermOrdKey, &'a Value);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some((key, value)) = self.leaf.as_mut().and_then(Iterator::next) {
                return Some((key, value));
            }
            self.leaf = None;
            match self.stack.pop()? {
                MapNode::Leaf(entries) => self.leaf = Some(entries.iter()),
                MapNode::Branch { children, .. } => {
                    self.stack.extend(children.iter().rev().map(Shared::as_ref));
                }
            }
        }
    }
}

fn leaf(entries: Vec<Entry>) -> Shared<MapNode> {
    Shared::new(MapNode::Leaf(entries))
}

fn branch(children: Vec<Shared<MapNode>>) -> Shared<MapNode> {
    let mut node = MapNode::Branch {
        children,
        max_keys: Vec::new(),
        height: 1,
    };
    refresh(&mut node);
    Shared::new(node)
}

fn node_height(node: &MapNode) -> u16 {
    match node {
        MapNode::Leaf(_) => 1,
        MapNode::Branch { height, .. } => *height,
    }
}

fn max_key(node: &MapNode) -> Option<&TermOrdKey> {
    let mut cursor = node;
    loop {
        match cursor {
            MapNode::Leaf(entries) => return entries.last().map(|(key, _)| key),
            MapNode::Branch { children, .. } => cursor = children.last()?.as_ref(),
        }
    }
}

fn refresh(node: &mut MapNode) {
    let MapNode::Branch {
        children,
        max_keys,
        height,
    } = node
    else {
        return;
    };
    refresh_parts(children, max_keys, height);
}

fn refresh_parts(children: &[Shared<MapNode>], max_keys: &mut Vec<TermOrdKey>, height: &mut u16) {
    max_keys.clear();
    max_keys.extend(children.iter().filter_map(|child| max_key(child).cloned()));
    *height = 1 + children
        .iter()
        .map(|child| node_height(child))
        .max()
        .unwrap_or(0);
}

fn child_index(max_keys: &[TermOrdKey], key: &TermOrdKey, child_count: usize) -> Option<usize> {
    if child_count == 0 {
        return None;
    }
    let index = max_keys.partition_point(|candidate| candidate < key);
    Some(index.min(child_count - 1))
}

fn insert(root: &mut Shared<MapNode>, key: TermOrdKey, value: Value) -> (bool, Link) {
    match Shared::make_mut(root) {
        MapNode::Leaf(entries) => {
            let inserted = match entries.binary_search_by(|(candidate, _)| candidate.cmp(&key)) {
                Ok(index) => {
                    if let Some((_, current)) = entries.get_mut(index) {
                        *current = value;
                    }
                    false
                }
                Err(index) => {
                    entries.insert(index, (key, value));
                    true
                }
            };
            let overflow =
                (entries.len() > PAGE_CAPACITY).then(|| leaf(entries.split_off(entries.len() / 2)));
            (inserted, overflow)
        }
        MapNode::Branch {
            children,
            max_keys,
            height,
        } => {
            if children.is_empty() {
                children.push(leaf(vec![(key, value)]));
                refresh_parts(children, max_keys, height);
                return (true, None);
            }
            let Some(index) = child_index(max_keys, &key, children.len()) else {
                return (false, None);
            };
            let Some(child) = children.get_mut(index) else {
                return (false, None);
            };
            let (inserted, child_overflow) = insert(child, key, value);
            if let Some(sibling) = child_overflow {
                children.insert(index + 1, sibling);
            }
            let overflow_children =
                (children.len() > PAGE_CAPACITY).then(|| children.split_off(children.len() / 2));
            refresh_parts(children, max_keys, height);
            (inserted, overflow_children.map(branch))
        }
    }
}

impl Finalize for MapNode {}

unsafe impl Trace for MapNode {
    fn trace(&self, ctx: &mut Context<'_>) {
        match self {
            Self::Leaf(entries) => {
                for (_, value) in entries {
                    value.trace(ctx);
                }
            }
            Self::Branch { children, .. } => children.trace(ctx),
        }
    }
}
