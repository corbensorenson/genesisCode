use super::Env;
use crate::Shared;
use std::collections::{BTreeMap, BTreeSet};

impl Env {
    /// Detach a closure from intermediate lexical frames while retaining the live module root.
    pub(crate) fn capture(&self, names: &BTreeSet<String>) -> Self {
        let root = self.module_anchor();
        let mut captures = BTreeMap::new();
        for name in names {
            let mut cur = Some(self.clone());
            while let Some(env) = cur {
                if Shared::ptr_eq(&env.0, &root.0) {
                    break;
                }
                if let Some(value) = env.0.binds.borrow().get(name).cloned() {
                    captures.insert(name.clone(), value);
                    break;
                }
                cur = env.0.parent.clone();
            }
        }
        if captures.is_empty() {
            root
        } else {
            Self::with_bindings(&root, captures)
        }
    }

    fn module_anchor(&self) -> Self {
        let mut current = self.clone();
        loop {
            if current.0.module_scope.get() {
                return current;
            }
            let Some(parent) = current.0.parent.clone() else {
                return current;
            };
            current = parent;
        }
    }

    #[cfg(test)]
    pub(crate) fn captured_local_binding_count(&self) -> usize {
        if self.0.parent.is_none() {
            0
        } else {
            self.0.binds.borrow().len()
        }
    }
}
