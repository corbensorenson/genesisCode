#[cfg(test)]
use std::collections::BTreeSet;

use rust_cc::{Context, Finalize, Trace};

use crate::{Shared, Value};

const LEAF_CAPACITY: usize = 32;
type Link = Option<Shared<VectorNode>>;

#[derive(Clone, Debug, Default)]
pub struct ValueVector {
    root: Link,
    len: usize,
}

#[derive(Clone, Debug)]
enum VectorNode {
    Leaf(Vec<Value>),
    Branch {
        left: Shared<VectorNode>,
        right: Shared<VectorNode>,
        len: usize,
        height: u16,
    },
}

impl ValueVector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, mut index: usize) -> Option<&Value> {
        if index >= self.len {
            return None;
        }
        let mut cursor = self.root.as_deref()?;
        loop {
            match cursor {
                VectorNode::Leaf(values) => return values.get(index),
                VectorNode::Branch { left, right, .. } => {
                    let left_len = node_len(left);
                    if index < left_len {
                        cursor = left;
                    } else {
                        index -= left_len;
                        cursor = right;
                    }
                }
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Value> {
        VectorIter::new(self.root.as_deref())
    }

    pub fn push_shared(vector: &mut Shared<Self>, value: Value) {
        let vector = Shared::make_mut(vector);
        match vector.root.as_mut() {
            None => vector.root = Some(leaf(vec![value])),
            Some(root) => {
                if let Some(overflow) = append(root, value) {
                    vector.root = Some(concat(root.clone(), overflow));
                }
            }
        }
        vector.len = vector.len.saturating_add(1);
    }

    pub fn set_shared(vector: &mut Shared<Self>, index: usize, value: Value) -> bool {
        if index >= vector.len {
            return false;
        }
        let vector = Shared::make_mut(vector);
        let Some(root) = vector.root.as_mut() else {
            return false;
        };
        set(root, index, value);
        true
    }

    pub(crate) fn trace_owners(&self, ctx: &mut Context<'_>) {
        self.root.trace(ctx);
    }

    #[cfg(test)]
    pub(crate) fn collect_node_identities(&self, identities: &mut BTreeSet<usize>) {
        let mut stack = self.root.iter().collect::<Vec<_>>();
        while let Some(node) = stack.pop() {
            let identity = node.identity_ptr() as usize;
            if !identities.insert(identity) {
                continue;
            }
            if let VectorNode::Branch { left, right, .. } = node.as_ref() {
                stack.push(left);
                stack.push(right);
            }
        }
    }
}

impl FromIterator<Value> for ValueVector {
    fn from_iter<T: IntoIterator<Item = Value>>(iter: T) -> Self {
        let mut leaves = Vec::new();
        let mut chunk = Vec::with_capacity(LEAF_CAPACITY);
        let mut len = 0usize;
        for value in iter {
            chunk.push(value);
            len = len.saturating_add(1);
            if chunk.len() == LEAF_CAPACITY {
                leaves.push(leaf(std::mem::take(&mut chunk)));
                chunk = Vec::with_capacity(LEAF_CAPACITY);
            }
        }
        if !chunk.is_empty() {
            leaves.push(leaf(chunk));
        }
        while leaves.len() > 1 {
            let mut parents = Vec::with_capacity(leaves.len().div_ceil(2));
            let mut nodes = leaves.into_iter();
            while let Some(left) = nodes.next() {
                parents.push(match nodes.next() {
                    Some(right) => branch(left, right),
                    None => left,
                });
            }
            leaves = parents;
        }
        Self {
            root: leaves.pop(),
            len,
        }
    }
}

struct VectorIter<'a> {
    stack: Vec<&'a VectorNode>,
    leaf: Option<std::slice::Iter<'a, Value>>,
}

impl<'a> VectorIter<'a> {
    fn new(root: Option<&'a VectorNode>) -> Self {
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

impl<'a> Iterator for VectorIter<'a> {
    type Item = &'a Value;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(value) = self.leaf.as_mut().and_then(Iterator::next) {
                return Some(value);
            }
            self.leaf = None;
            match self.stack.pop()? {
                VectorNode::Leaf(values) => self.leaf = Some(values.iter()),
                VectorNode::Branch { left, right, .. } => {
                    self.stack.push(right);
                    self.stack.push(left);
                }
            }
        }
    }
}

fn node_len(node: &VectorNode) -> usize {
    match node {
        VectorNode::Leaf(values) => values.len(),
        VectorNode::Branch { len, .. } => *len,
    }
}

fn height(node: &VectorNode) -> u16 {
    match node {
        VectorNode::Leaf(_) => 1,
        VectorNode::Branch { height, .. } => *height,
    }
}

fn leaf(values: Vec<Value>) -> Shared<VectorNode> {
    Shared::new(VectorNode::Leaf(values))
}

fn branch(left: Shared<VectorNode>, right: Shared<VectorNode>) -> Shared<VectorNode> {
    Shared::new(VectorNode::Branch {
        len: node_len(&left).saturating_add(node_len(&right)),
        height: 1 + height(&left).max(height(&right)),
        left,
        right,
    })
}

fn concat(left: Shared<VectorNode>, right: Shared<VectorNode>) -> Shared<VectorNode> {
    let left_height = height(&left);
    let right_height = height(&right);
    if left_height > right_height.saturating_add(1) {
        let VectorNode::Branch {
            left: outer_left,
            right: inner_left,
            ..
        } = left.as_ref()
        else {
            return branch(left, right);
        };
        return branch(outer_left.clone(), concat(inner_left.clone(), right));
    }
    if right_height > left_height.saturating_add(1) {
        let VectorNode::Branch {
            left: inner_right,
            right: outer_right,
            ..
        } = right.as_ref()
        else {
            return branch(left, right);
        };
        return branch(concat(left, inner_right.clone()), outer_right.clone());
    }
    branch(left, right)
}

fn append(root: &mut Shared<VectorNode>, value: Value) -> Option<Shared<VectorNode>> {
    match Shared::make_mut(root) {
        VectorNode::Leaf(values) if values.len() < LEAF_CAPACITY => {
            values.push(value);
            None
        }
        VectorNode::Leaf(_) => Some(leaf(vec![value])),
        VectorNode::Branch {
            left,
            right,
            len,
            height: branch_height,
        } => {
            let overflow = append(right, value);
            *len = node_len(left).saturating_add(node_len(right));
            *branch_height = 1 + height(left).max(height(right));
            overflow
        }
    }
}

fn set(root: &mut Shared<VectorNode>, index: usize, value: Value) {
    match Shared::make_mut(root) {
        VectorNode::Leaf(values) => {
            values[index] = value;
        }
        VectorNode::Branch { left, right, .. } => {
            let left_len = node_len(left);
            if index < left_len {
                set(left, index, value);
            } else {
                set(right, index - left_len, value);
            }
        }
    }
}

impl Finalize for VectorNode {}

unsafe impl Trace for VectorNode {
    fn trace(&self, ctx: &mut Context<'_>) {
        match self {
            Self::Leaf(values) => values.trace(ctx),
            Self::Branch { left, right, .. } => {
                left.trace(ctx);
                right.trace(ctx);
            }
        }
    }
}
