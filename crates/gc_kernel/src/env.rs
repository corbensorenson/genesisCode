use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::Value;

mod capture;

#[derive(Clone, Debug)]
pub struct Env(pub(crate) Rc<EnvFrame>);

#[derive(Debug)]
pub struct EnvFrame {
    pub parent: Option<Env>,
    // Interior mutability allows top-level `def` to behave like a recursive module scope:
    // closures capture an `Env`, and later defs become visible without rebuilding env chains.
    pub binds: RefCell<BTreeMap<String, Value>>,
    pub rev: Cell<u64>,
    module_scope: Cell<bool>,
}

impl Env {
    pub fn empty() -> Self {
        Self(Rc::new(EnvFrame {
            parent: None,
            binds: RefCell::new(BTreeMap::new()),
            rev: Cell::new(0),
            module_scope: Cell::new(false),
        }))
    }

    pub fn with_binding(parent: &Env, name: impl Into<String>, val: Value) -> Self {
        let mut binds = BTreeMap::new();
        binds.insert(name.into(), val);
        Self(Rc::new(EnvFrame {
            parent: Some(parent.clone()),
            binds: RefCell::new(binds),
            rev: Cell::new(0),
            module_scope: Cell::new(false),
        }))
    }

    pub fn with_bindings(parent: &Env, new_binds: BTreeMap<String, Value>) -> Self {
        Self(Rc::new(EnvFrame {
            parent: Some(parent.clone()),
            binds: RefCell::new(new_binds),
            rev: Cell::new(0),
            module_scope: Cell::new(false),
        }))
    }

    pub fn set_local(&mut self, name: impl Into<String>, val: Value) {
        self.0.binds.borrow_mut().insert(name.into(), val);
        self.0.rev.set(self.0.rev.get().wrapping_add(1));
    }

    pub(crate) fn mark_module_scope(&self) {
        self.0.module_scope.set(true);
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        let mut cur: Option<&EnvFrame> = Some(self.0.as_ref());
        while let Some(frame) = cur {
            if let Some(v) = frame.binds.borrow().get(name) {
                return Some(v.clone());
            }
            cur = frame.parent.as_ref().map(|e| e.0.as_ref());
        }
        None
    }
}
