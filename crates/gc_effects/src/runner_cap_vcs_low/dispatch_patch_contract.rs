use super::*;

#[path = "dispatch_patch_contract/merge_ops.rs"]
mod merge_ops;
#[path = "dispatch_patch_contract/resolve_conflict.rs"]
mod resolve_conflict;

#[expect(
    clippy::too_many_arguments,
    reason = "capability dispatch signatures are explicit by design"
)]
pub(super) fn dispatch_patch_contract(
    op_eff: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    policy: &CapsPolicy,
    store: Option<&ArtifactStore>,
    refs: Option<&RefsDb>,
    budget: &mut ArtifactBudgetState,
    error_tok: SealId,
    op: &str,
    timeout_ms: Option<u64>,
) -> Result<Value, EffectsError> {
    let _ = (policy, refs, timeout_ms);
    match op_eff {
        "core/vcs-low::diff-terms" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/vcs-low::diff-terms".to_string())
            })?;
            let Term::Map(m) = payload else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "payload must be a map".to_string(),
                    Some(op),
                ));
            };
            let Some(base_t) = m.get(&TermOrdKey(Term::symbol(":base-term"))) else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "missing :base-term".to_string(),
                    Some(op),
                ));
            };
            let Some(to_t) = m.get(&TermOrdKey(Term::symbol(":to-term"))) else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "missing :to-term".to_string(),
                    Some(op),
                ));
            };
            let (patch_term, values) = match vcs_diff_patch_term(store, base_t, to_t) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/diff-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let mut out = BTreeMap::new();
            out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            out.insert(TermOrdKey(Term::symbol(":patch-term")), patch_term);
            out.insert(
                TermOrdKey(Term::symbol(":values")),
                Term::Vector(values.into_iter().map(Term::Str).collect()),
            );
            Ok(Value::data(Term::Map(out)))
        }
        "core/vcs-low::apply-patch" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log(
                    "missing artifact store for core/vcs-low::apply-patch".to_string(),
                )
            })?;
            let Term::Map(m) = payload else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "payload must be a map".to_string(),
                    Some(op),
                ));
            };
            let Some(base_t) = m.get(&TermOrdKey(Term::symbol(":base-term"))) else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "missing :base-term".to_string(),
                    Some(op),
                ));
            };
            let Some(patch_t) = m.get(&TermOrdKey(Term::symbol(":patch-term"))) else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "missing :patch-term".to_string(),
                    Some(op),
                ));
            };
            let patch = match gc_vcs::Patch::from_term(patch_t) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-patch",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let snapshot_t = match vcs_apply_patch_term(store, base_t, &patch) {
                Ok(t) => t,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/vcs/apply-error", e, Some(op)));
                }
            };
            let mut out = BTreeMap::new();
            out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            out.insert(TermOrdKey(Term::symbol(":snapshot-term")), snapshot_t);
            Ok(Value::data(Term::Map(out)))
        }
        "core/vcs-low::merge3-contract-snapshots" => Ok(
            merge_ops::handle_merge3_contract_snapshots(payload, error_tok, op),
        ),
        "core/vcs-low::resolve-conflict" => resolve_conflict::handle_resolve_conflict(
            payload, pol, policy, store, budget, error_tok, op,
        ),
        _ => Ok(mk_error(
            error_tok,
            "core/caps/unknown-op-eff",
            format!("core/vcs-low dispatch received unsupported op_eff: {op_eff}"),
            Some(op),
        )),
    }
}
