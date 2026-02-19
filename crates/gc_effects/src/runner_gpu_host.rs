use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey};
use gc_kernel::{SealId, Value};

use crate::policy::OpPolicy;
use crate::runner_host_bridge::{BridgeError, call_host_bridge};

#[derive(Debug, Clone, Default)]
pub(crate) struct GpuHostRuntime;

pub(crate) fn gpu_host_call(
    _runtime: &mut GpuHostRuntime,
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Option<Value> {
    if !is_gpu_host_op(op) {
        return None;
    }
    Some(match call_host_bridge("gpu", op, payload, pol) {
        Ok(resp) => Value::Data(resp),
        Err(err) => mk_error(error_tok, &err, Some(op)),
    })
}

fn is_gpu_host_op(op: &str) -> bool {
    matches!(
        op,
        "gfx/gpu::create-buffer"
            | "gpu/compute::create-buffer"
            | "gfx/gpu::create-texture"
            | "gfx/gpu::create-sampler"
            | "gfx/gpu::create-shader-module"
            | "gpu/compute::create-shader-module"
            | "gfx/gpu::create-bind-group-layout"
            | "gpu/compute::create-bind-group-layout"
            | "gfx/gpu::create-bind-group"
            | "gpu/compute::create-bind-group"
            | "gfx/gpu::create-pipeline-layout"
            | "gpu/compute::create-pipeline-layout"
            | "gfx/gpu::create-render-pipeline"
            | "gfx/gpu::create-compute-pipeline"
            | "gpu/compute::create-compute-pipeline"
            | "gpu/compute::create-kernel"
            | "gfx/gpu::destroy-resource"
            | "gpu/compute::destroy-resource"
            | "gfx/gpu::write-buffer"
            | "gpu/compute::write-buffer"
            | "gfx/gpu::write-texture"
            | "gfx/gpu::read-buffer"
            | "gpu/compute::read-buffer"
            | "gfx/gpu::read-texture"
            | "gfx/gpu::submit-frame-graph"
            | "gfx/gpu::submit-compute-graph"
            | "gpu/compute::submit"
            | "gfx/gpu::limits"
            | "gpu/compute::limits"
            | "gfx/gpu::features"
            | "gpu/compute::features"
    )
}

fn mk_error(error_tok: SealId, err: &BridgeError, op: Option<&str>) -> Value {
    let mut mm = BTreeMap::new();
    mm.insert(
        TermOrdKey(Term::symbol(":error/code")),
        Term::Str(err.code.clone()),
    );
    mm.insert(
        TermOrdKey(Term::symbol(":error/message")),
        Term::Str(err.message.clone()),
    );
    mm.insert(
        TermOrdKey(Term::symbol(":error/op")),
        op.map(Term::symbol).unwrap_or(Term::Nil),
    );
    Value::Sealed {
        token: error_tok,
        payload: Box::new(Value::Data(Term::Map(mm))),
    }
}
