use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey};
use gc_kernel::{SealId, Value};

use crate::policy::OpPolicy;
use crate::runner_host_bridge::{BridgeError, call_host_bridge};

#[derive(Debug, Clone, Default)]
pub(crate) struct GfxHostRuntime;

pub(crate) fn gfx_host_call(
    _runtime: &mut GfxHostRuntime,
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Option<Value> {
    if !is_gfx_host_op(op) {
        return None;
    }
    Some(match call_host_bridge("gfx", op, payload, pol) {
        Ok(resp) => Value::Data(resp),
        Err(err) => mk_error(error_tok, &err, Some(op)),
    })
}

fn is_gfx_host_op(op: &str) -> bool {
    matches!(
        op,
        "gfx/window::create-surface"
            | "gfx/window::resize-surface"
            | "gfx/window::set-title"
            | "gfx/window::request-redraw"
            | "gfx/window::surface-info"
            | "gfx/input::poll-events"
            | "gfx/input::set-cursor-mode"
            | "gfx/audio::enqueue"
            | "gfx/audio::set-master"
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
