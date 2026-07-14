use serde_json::Value;

use super::*;

pub(super) fn cancel_request(params: &Value, state: &mut State) -> Option<PendingCall> {
    let target = params.get("requestId").filter(|id| valid_rpc_atom(id))?;
    let key = rpc_key(target);
    if let Some(position) = state.pending.iter().position(|pending| pending.key == key) {
        if let Some(pending) = state.pending.remove(position) {
            state.active_ids.remove(&pending.key);
            return Some(pending);
        }
    } else if let Some(running) = state.running.as_mut().filter(|running| running.key == key) {
        running.cancelled = true;
        if let Some(control) = &running.control {
            control.cancel();
        }
    }
    None
}
