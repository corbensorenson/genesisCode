use std::collections::BTreeMap;
use std::rc::Rc;

use crate::Value;

#[derive(Clone, Debug)]
pub struct Env(pub(crate) Rc<EnvFrame>);

#[derive(Debug)]
pub struct EnvFrame {
    pub parent: Option<Env>,
    pub binds: BTreeMap<String, Value>,
}

impl Env {
    pub fn empty() -> Self {
        Self(Rc::new(EnvFrame {
            parent: None,
            binds: BTreeMap::new(),
        }))
    }

    pub fn with_binding(parent: &Env, name: impl Into<String>, val: Value) -> Self {
        let mut binds = BTreeMap::new();
        binds.insert(name.into(), val);
        Self(Rc::new(EnvFrame {
            parent: Some(parent.clone()),
            binds,
        }))
    }

    pub fn with_bindings(parent: &Env, new_binds: BTreeMap<String, Value>) -> Self {
        Self(Rc::new(EnvFrame {
            parent: Some(parent.clone()),
            binds: new_binds,
        }))
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        let mut cur: Option<&EnvFrame> = Some(self.0.as_ref());
        while let Some(frame) = cur {
            if let Some(v) = frame.binds.get(name) {
                return Some(v.clone());
            }
            cur = frame.parent.as_ref().map(|e| e.0.as_ref());
        }
        None
    }
}
