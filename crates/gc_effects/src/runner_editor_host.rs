use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey};
use gc_kernel::{SealId, Value};

use crate::policy::OpPolicy;
use crate::runner_host_bridge::{BridgeError, call_host_bridge};

#[derive(Debug, Clone, Default)]
pub(crate) struct EditorHostRuntime;

pub(crate) fn editor_host_call(
    _runtime: &mut EditorHostRuntime,
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Option<Value> {
    if !is_editor_host_op(op) {
        return None;
    }
    Some(match call_host_bridge("editor", op, payload, pol) {
        Ok(resp) => Value::Data(resp),
        Err(err) => mk_error(error_tok, &err, Some(op)),
    })
}

fn is_editor_host_op(op: &str) -> bool {
    matches!(
        op,
        "editor/clipboard::get"
            | "editor/clipboard::set"
            | "editor/dialog::open"
            | "editor/dialog::save"
            | "editor/plugin::command"
            | "editor/watch::subscribe"
            | "editor/watch::poll"
            | "editor/watch::unsubscribe"
            | "editor/task::spawn"
            | "editor/task::poll"
            | "editor/task::cancel"
            | "editor/task::fmt-coreform"
            | "editor/task::lint-module"
            | "editor/task::optimize-module"
            | "editor/task::parse-module"
            | "editor/task::test-pkg"
            | "editor/task::typecheck-pkg"
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
