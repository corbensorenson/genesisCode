use super::*;

#[derive(Clone, Debug)]
pub struct CompiledLexicalEnv {
    pub(super) values: Shared<Vec<(u16, Value)>>,
    names: Arc<BTreeMap<String, usize>>,
}

impl CompiledLexicalEnv {
    pub(super) fn empty() -> Self {
        Self {
            values: Shared::new(Vec::new()),
            names: Arc::default(),
        }
    }

    pub(super) fn get(&self, depth: u16, slot: u16) -> Option<Value> {
        if slot != 0 {
            return None;
        }
        self.value_at(usize::from(depth)).cloned()
    }

    pub(super) fn from_slots(
        slots: Vec<(u16, Value)>,
        names: Arc<BTreeMap<String, usize>>,
    ) -> Self {
        Self {
            values: Shared::new(slots),
            names,
        }
    }

    fn value_at(&self, depth: usize) -> Option<&Value> {
        let depth = u16::try_from(depth).ok()?;
        let index = self
            .values
            .binary_search_by_key(&depth, |(slot, _)| *slot)
            .ok()?;
        Some(&self.values[index].1)
    }

    pub(crate) fn named_bindings(&self) -> impl DoubleEndedIterator<Item = (&str, &Value)> {
        self.names
            .iter()
            .filter_map(|(name, slot)| self.value_at(*slot).map(|value| (name.as_str(), value)))
    }

    pub(crate) fn has_named_binding(&self, name: &str) -> bool {
        self.names.contains_key(name)
    }

    pub(crate) fn has_captures(&self) -> bool {
        !self.names.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn captured_value_count(&self) -> usize {
        self.values.len()
    }

    #[cfg(test)]
    pub(crate) fn slot_span(&self) -> usize {
        self.values
            .last()
            .map_or(0, |(depth, _)| usize::from(*depth) + 1)
    }
}

#[derive(Clone, Debug)]
pub struct CompiledModuleCells {
    // Slots resolve through the same live named scope as the reference tier.
    // A separate value table becomes stale when a warm caller defines a name again.
    names: Arc<[String]>,
    pub(super) bindings: Env,
}

impl CompiledModuleCells {
    pub(super) fn new(names: &[String], bindings: Env) -> Self {
        Self {
            names: Arc::from(names),
            bindings,
        }
    }

    pub(super) fn empty() -> Self {
        Self::new(&[], Env::empty())
    }

    pub(super) fn get(&self, slot: u32) -> Option<Value> {
        let slot = usize::try_from(slot).ok()?;
        self.bindings.get(self.names.get(slot)?)
    }

    pub(super) fn set(&self, slot: u32, value: Value) -> Result<(), KernelError> {
        let slot = usize::try_from(slot).map_err(|_| {
            KernelError::new(KernelErrorKind::Internal, "module slot exceeds usize range")
        })?;
        let Some(name) = self.names.get(slot) else {
            return Err(KernelError::new(
                KernelErrorKind::Internal,
                format!("module slot out of range: {slot}"),
            ));
        };
        self.bindings.clone().set_local(name.clone(), value);
        Ok(())
    }
}
