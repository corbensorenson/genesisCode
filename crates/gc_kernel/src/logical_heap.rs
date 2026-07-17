use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use gc_coreform::Term;

use crate::Shared;
use crate::env::{Env, EnvFrame};
use crate::value::{Contract, Value};

#[derive(Debug)]
pub(crate) struct AllocationLedger {
    allocated: AtomicU64,
    limit: AtomicU64,
    has_limit: AtomicBool,
}

impl AllocationLedger {
    pub(crate) fn new() -> Self {
        Self {
            allocated: AtomicU64::new(0),
            limit: AtomicU64::new(0),
            has_limit: AtomicBool::new(false),
        }
    }

    pub(crate) fn set_limit(&self, limit: Option<u64>) {
        if let Some(limit) = limit {
            self.limit.store(limit, Ordering::Relaxed);
            self.has_limit.store(true, Ordering::Release);
        } else {
            self.has_limit.store(false, Ordering::Release);
        }
    }

    pub(crate) fn reset(&self) {
        self.allocated.store(0, Ordering::Relaxed);
    }

    pub(crate) fn observed(&self) -> u64 {
        self.allocated.load(Ordering::Relaxed)
    }

    pub(crate) fn limit(&self) -> Option<u64> {
        if self.has_limit.load(Ordering::Acquire) {
            Some(self.limit.load(Ordering::Relaxed))
        } else {
            None
        }
    }

    fn charge(&self, units: u64) {
        let mut observed = self.allocated.load(Ordering::Relaxed);
        loop {
            let next = observed.saturating_add(units);
            match self.allocated.compare_exchange_weak(
                observed,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(actual) => observed = actual,
            }
        }
    }
}

thread_local! {
    static ACTIVE_LEDGERS: RefCell<Vec<Arc<AllocationLedger>>> = const { RefCell::new(Vec::new()) };
}

pub(crate) struct ActiveAllocationGuard;

impl ActiveAllocationGuard {
    pub(crate) fn enter(ledger: Arc<AllocationLedger>) -> Self {
        ACTIVE_LEDGERS.with(|active| active.borrow_mut().push(ledger));
        Self
    }
}

impl Drop for ActiveAllocationGuard {
    fn drop(&mut self) {
        ACTIVE_LEDGERS.with(|active| {
            active.borrow_mut().pop();
        });
    }
}

pub(crate) fn charge_active(units: u64) {
    if units == 0 {
        return;
    }
    ACTIVE_LEDGERS.with(|active| {
        if let Some(ledger) = active.borrow().last() {
            ledger.charge(units);
        }
    });
}

pub(crate) fn charge_active_with(units: impl FnOnce() -> u64) {
    ACTIVE_LEDGERS.with(|active| {
        if let Some(ledger) = active.borrow().last() {
            ledger.charge(units());
        }
    });
}

pub(crate) fn data_allocation_units(term: &Term) -> u64 {
    2u64.saturating_add(logical_term_units(term))
}

pub(crate) fn vector_allocation_units(len: usize) -> u64 {
    1u64.saturating_add(len as u64)
}

pub(crate) fn map_allocation_units<'a>(
    entries: impl Iterator<Item = &'a gc_coreform::TermOrdKey>,
) -> u64 {
    entries.fold(1u64, |units, key| {
        units
            .saturating_add(2)
            .saturating_add(logical_term_units(&key.0))
    })
}

pub(crate) fn closure_allocation_units(param: &str, body: &Term) -> u64 {
    3u64.saturating_add(param.len() as u64)
        .saturating_add(logical_term_units(body))
}

pub(crate) fn native_allocation_units(name: &str, collected: usize) -> u64 {
    1u64.saturating_add(name.len() as u64)
        .saturating_add(collected as u64)
}

pub(crate) fn contract_allocation_units(contract: &Contract) -> u64 {
    let base = 3u64.saturating_add(u64::from(contract.proto.is_some()));
    contract.overrides.iter().fold(base, |units, (name, _)| {
        units.saturating_add(1).saturating_add(name.len() as u64)
    })
}

pub(crate) fn effect_request_allocation_units(op: &str, payload: &Term) -> u64 {
    3u64.saturating_add(op.len() as u64)
        .saturating_add(logical_term_units(payload))
}

pub(crate) fn logical_term_units(root: &Term) -> u64 {
    let mut units = 0u64;
    let mut stack = vec![root];
    while let Some(term) = stack.pop() {
        units = units.saturating_add(1);
        match term {
            Term::Nil | Term::Bool(_) => {}
            Term::Int(value) => {
                units = units.saturating_add(value.to_signed_bytes_le().len() as u64);
            }
            Term::Str(value) | Term::Symbol(value) => {
                units = units.saturating_add(value.len() as u64);
            }
            Term::Bytes(value) => {
                units = units.saturating_add(value.len() as u64);
            }
            Term::Pair(left, right) => {
                units = units.saturating_add(2);
                stack.push(left);
                stack.push(right);
            }
            Term::Vector(values) => {
                units = units.saturating_add(values.len() as u64);
                stack.extend(values);
            }
            Term::Map(entries) => {
                units = units.saturating_add((entries.len() as u64).saturating_mul(2));
                for (key, value) in entries {
                    stack.push(&key.0);
                    stack.push(value);
                }
            }
        }
    }
    units
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum CycleKey {
    Vector(*const crate::value::ValueVector),
    Map(*const crate::value::ValueMap),
    Closure(*const crate::value::ClosureData),
    CompiledClosure(*const crate::value::CompiledClosureData),
    Native(*const crate::value::NativeFn),
    Contract(*const Contract),
    EffectRequest(*const crate::value::EffectRequest),
    Env(*const EnvFrame),
}

enum Work {
    Value(Value),
    Env(Env),
    Contract(Shared<Contract>),
    Exit(CycleKey),
}

fn push_edge(stack: &mut Vec<Work>, work: Work, units: &mut u64) {
    *units = units.saturating_add(1);
    stack.push(work);
}

fn charge_term_edge(term: &Term, units: &mut u64) {
    *units = units
        .saturating_add(1)
        .saturating_add(logical_term_units(term));
}

fn enter_cycle(active: &mut HashSet<CycleKey>, stack: &mut Vec<Work>, key: CycleKey) -> bool {
    if !active.insert(key) {
        return false;
    }
    stack.push(Work::Exit(key));
    true
}

pub(crate) fn logical_live_units(values: &[&Value], environments: &[&Env]) -> u64 {
    let mut units = 0u64;
    let mut active = HashSet::new();
    let mut stack = Vec::new();
    for value in values.iter().rev() {
        push_edge(&mut stack, Work::Value((*value).clone()), &mut units);
    }
    for environment in environments.iter().rev() {
        push_edge(&mut stack, Work::Env((*environment).clone()), &mut units);
    }

    while let Some(work) = stack.pop() {
        match work {
            Work::Exit(key) => {
                active.remove(&key);
            }
            Work::Env(env) => {
                let key = CycleKey::Env(env.0.identity_ptr());
                if !enter_cycle(&mut active, &mut stack, key) {
                    continue;
                }
                units = units.saturating_add(1);
                if let Some(parent) = &env.0.parent {
                    push_edge(&mut stack, Work::Env(parent.clone()), &mut units);
                }
                let bindings = env.0.binds.borrow();
                for (name, value) in bindings.iter().rev() {
                    units = units.saturating_add(name.len() as u64);
                    push_edge(&mut stack, Work::Value(value.clone()), &mut units);
                }
            }
            Work::Contract(contract) => {
                push_edge(
                    &mut stack,
                    Work::Value(contract.handler.clone()),
                    &mut units,
                );
                push_edge(&mut stack, Work::Value(contract.meta.clone()), &mut units);
                if let Some(proto) = &contract.proto {
                    push_edge(&mut stack, Work::Contract(proto.clone()), &mut units);
                }
                for (name, value) in contract.overrides.iter().rev() {
                    units = units.saturating_add(name.len() as u64);
                    push_edge(&mut stack, Work::Value(value.clone()), &mut units);
                }
            }
            Work::Value(value) => {
                units = units.saturating_add(1);
                match value {
                    Value::Data(term) => {
                        charge_term_edge(&term, &mut units);
                    }
                    Value::Int(_) | Value::SealToken(_) => {}
                    Value::Vector(values) => {
                        let key = CycleKey::Vector(values.identity_ptr());
                        if !enter_cycle(&mut active, &mut stack, key) {
                            continue;
                        }
                        for value in values.iter() {
                            push_edge(&mut stack, Work::Value(value.clone()), &mut units);
                        }
                    }
                    Value::Map(entries) => {
                        let key = CycleKey::Map(entries.identity_ptr());
                        if !enter_cycle(&mut active, &mut stack, key) {
                            continue;
                        }
                        for (term, value) in entries.iter() {
                            charge_term_edge(&term.0, &mut units);
                            push_edge(&mut stack, Work::Value(value.clone()), &mut units);
                        }
                    }
                    Value::Closure(closure) => {
                        let key = CycleKey::Closure(closure.identity_ptr());
                        if !enter_cycle(&mut active, &mut stack, key) {
                            continue;
                        }
                        units = units.saturating_add(closure.param.len() as u64);
                        charge_term_edge(&closure.body, &mut units);
                        push_edge(&mut stack, Work::Env(closure.env.clone()), &mut units);
                    }
                    Value::CompiledClosure(closure) => {
                        let key = CycleKey::CompiledClosure(closure.identity_ptr());
                        if !enter_cycle(&mut active, &mut stack, key) {
                            continue;
                        }
                        units = units.saturating_add(closure.param.len() as u64);
                        charge_term_edge(&closure.body, &mut units);
                        push_edge(&mut stack, Work::Env(closure.env.clone()), &mut units);
                    }
                    Value::Sealed { payload, .. } => {
                        push_edge(&mut stack, Work::Value(*payload), &mut units);
                    }
                    Value::NativeFn(native) => {
                        let key = CycleKey::Native(native.identity_ptr());
                        if !enter_cycle(&mut active, &mut stack, key) {
                            continue;
                        }
                        units = units.saturating_add(native.name.len() as u64);
                        for value in native.collected.iter().rev() {
                            push_edge(&mut stack, Work::Value(value.clone()), &mut units);
                        }
                    }
                    Value::Contract(contract) => {
                        let key = CycleKey::Contract(contract.identity_ptr());
                        if !enter_cycle(&mut active, &mut stack, key) {
                            continue;
                        }
                        stack.push(Work::Contract(contract));
                    }
                    Value::EffectProgram(program) => match program.as_ref() {
                        crate::value::EffectProgram::Pure(value) => {
                            push_edge(&mut stack, Work::Value((**value).clone()), &mut units);
                        }
                        crate::value::EffectProgram::Perform { request } => {
                            push_edge(&mut stack, Work::Value((**request).clone()), &mut units);
                        }
                    },
                    Value::EffectRequest(request) => {
                        let key = CycleKey::EffectRequest(request.identity_ptr());
                        if !enter_cycle(&mut active, &mut stack, key) {
                            continue;
                        }
                        units = units.saturating_add(request.op.len() as u64);
                        charge_term_edge(&request.payload, &mut units);
                        push_edge(&mut stack, Work::Value((*request.k).clone()), &mut units);
                    }
                }
            }
        }
    }
    units
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use num_bigint::BigInt;

    use super::*;
    use crate::ValueVector;

    #[test]
    fn logical_term_and_constructor_units_are_exact() {
        assert_eq!(logical_term_units(&Term::Nil), 1);
        assert_eq!(logical_term_units(&Term::Int(BigInt::from(1))), 2);
        assert_eq!(
            logical_term_units(&Term::Pair(Box::new(Term::Nil), Box::new(Term::Bool(true)))),
            5
        );

        let ledger = Arc::new(AllocationLedger::new());
        ledger.set_limit(Some(u64::MAX));
        assert_eq!(ledger.limit(), Some(u64::MAX));
        {
            let _guard = ActiveAllocationGuard::enter(ledger.clone());
            let nil = Value::data(Term::Nil);
            let _vector = Value::vector(ValueVector::from_iter([nil]));
            let _integer = Value::int(1);
        }
        assert_eq!(ledger.observed(), 6);
    }

    #[test]
    fn live_units_are_alias_representation_independent() {
        let item = Value::data(Term::Nil);
        let shared = Value::vector(ValueVector::from_iter([item.clone(), item]));
        let copied = Value::vector(ValueVector::from_iter([
            Value::data(Term::Nil),
            Value::data(Term::Nil),
        ]));
        assert_eq!(logical_live_units(&[&shared], &[]), 10);
        assert_eq!(
            logical_live_units(&[&shared], &[]),
            logical_live_units(&[&copied], &[])
        );
    }

    #[test]
    fn live_units_terminate_on_recursive_environment_cycles() {
        let mut env = Env::empty();
        let closure = Value::closure("x".to_string(), Term::symbol("x"), env.clone());
        env.set_local("f", closure.clone());
        assert_eq!(logical_live_units(&[&closure], &[]), 11);
        drop(closure);
        drop(env);
        crate::cycle::collect_cycles();
    }
}
