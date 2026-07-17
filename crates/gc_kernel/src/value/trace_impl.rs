use rust_cc::{Context, Finalize, Trace};

use super::{
    ClosureData, CompiledClosureData, Contract, EffectProgram, EffectRequest, NativeFn, Value,
    ValueMap, ValueVector,
};

macro_rules! empty_finalize {
    ($($ty:ty),+ $(,)?) => { $(impl Finalize for $ty {})+ };
}

empty_finalize!(
    ValueMap,
    ValueVector,
    Value,
    ClosureData,
    CompiledClosureData,
    NativeFn,
    Contract,
    EffectProgram,
    EffectRequest,
);

unsafe impl Trace for ValueMap {
    fn trace(&self, ctx: &mut Context<'_>) {
        for value in self.0.values() {
            value.trace(ctx);
        }
    }
}

unsafe impl Trace for ValueVector {
    fn trace(&self, ctx: &mut Context<'_>) {
        match self {
            Self::Flat(values) => values.trace(ctx),
        }
    }
}

unsafe impl Trace for Value {
    fn trace(&self, ctx: &mut Context<'_>) {
        match self {
            Self::Vector(value) => value.trace(ctx),
            Self::Map(value) => value.trace(ctx),
            Self::Closure(value) => value.trace(ctx),
            Self::CompiledClosure(value) => value.trace(ctx),
            Self::Sealed { payload, .. } => payload.trace(ctx),
            Self::NativeFn(value) => value.trace(ctx),
            Self::Contract(value) => value.trace(ctx),
            Self::EffectProgram(value) => value.trace(ctx),
            Self::EffectRequest(value) => value.trace(ctx),
            Self::Data(_) | Self::Int(_) | Self::SealToken(_) => {}
        }
    }
}

unsafe impl Trace for ClosureData {
    fn trace(&self, ctx: &mut Context<'_>) {
        self.env.trace(ctx);
    }
}

unsafe impl Trace for CompiledClosureData {
    fn trace(&self, ctx: &mut Context<'_>) {
        self.env.trace(ctx);
        self.compiled_env.trace(ctx);
        self.module_env.trace(ctx);
    }
}

unsafe impl Trace for NativeFn {
    fn trace(&self, ctx: &mut Context<'_>) {
        self.collected.trace(ctx);
    }
}

unsafe impl Trace for Contract {
    fn trace(&self, ctx: &mut Context<'_>) {
        self.handler.trace(ctx);
        self.proto.trace(ctx);
        self.meta.trace(ctx);
        for value in self.overrides.values() {
            value.trace(ctx);
        }
    }
}

unsafe impl Trace for EffectProgram {
    fn trace(&self, ctx: &mut Context<'_>) {
        match self {
            Self::Pure(value) => value.trace(ctx),
            Self::Perform { request } => request.trace(ctx),
        }
    }
}

unsafe impl Trace for EffectRequest {
    fn trace(&self, ctx: &mut Context<'_>) {
        self.k.trace(ctx);
    }
}
