use serde_json::Value;

use super::*;

pub(super) fn cancel_request(params: &Value, state: &mut State) {
    let Some(target) = params.get("requestId").filter(|id| valid_rpc_atom(id)) else {
        return;
    };
    let key = rpc_key(target);
    if let Some(position) = state.pending.iter().position(|pending| pending.key == key) {
        if let Some(pending) = state.pending.remove(position) {
            state.active_ids.remove(&pending.key);
        }
    } else if let Some(running) = state.running.as_mut().filter(|running| running.key == key) {
        running.cancelled = true;
    }
}

pub(super) fn cancel_all(state: &mut State) {
    for pending in state.pending.drain(..) {
        state.active_ids.remove(&pending.key);
    }
    if let Some(running) = state.running.as_mut() {
        running.cancelled = true;
    }
}
