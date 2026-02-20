use super::*;

#[expect(
    clippy::too_many_arguments,
    reason = "capability dispatch signatures are explicit by design"
)]
pub(super) fn dispatch_publish(
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
    let _ = timeout_ms;
    match op_eff {
        "core/pkg-low::snapshot" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg-low::snapshot".to_string())
            })?;
            handle_snapshot(payload, pol, policy, store, budget, error_tok, op)
        }

        "core/pkg-low::publish" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg-low::publish".to_string())
            })?;
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/pkg-low::publish".to_string())
            })?;

            let remote_s = match payload_pkg_publish_remote(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let refname = match payload_pkg_publish_ref(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let policy_h = match payload_pkg_publish_policy(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let expected_old = match payload_pkg_publish_expected_old(payload) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let depth = payload_pkg_publish_depth(payload).unwrap_or(0);
            let commit_override = match payload_pkg_publish_commit(payload) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };

            let commit_hex = if let Some(h) = commit_override {
                h
            } else {
                let h = match refs.get(&refname) {
                    Ok(h) => h,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/refs/io-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let Some(h) = h else {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/ref-not-found",
                        format!("local ref is unset: {refname}"),
                        Some(op),
                    ));
                };
                h
            };

            if gc_vcs::validate_hex_hash(&commit_hex).is_err() {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/bad-payload",
                    "commit must be 64-hex".to_string(),
                    Some(op),
                ));
            }
            if gc_vcs::validate_hex_hash(&policy_h).is_err() {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/bad-payload",
                    "policy must be 64-hex".to_string(),
                    Some(op),
                ));
            }

            let pol_term = match store_get_term(store, &policy_h) {
                Ok(t) => t,
                Err(_) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/not-found",
                        format!("artifact not found: {policy_h}"),
                        Some(op),
                    ));
                }
            };
            let pol_art = match gc_vcs::Policy::from_term(&pol_term) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-policy",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            if pol_art.is_frozen_ref(&refname) {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/ref-frozen",
                    format!("ref is frozen by policy: {refname}"),
                    Some(op),
                ));
            }
            let class = match pol_art.class_for_ref(&refname) {
                Some(c) => c,
                None => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/no-policy-class",
                        format!("policy has no matching class for ref: {refname}"),
                        Some(op),
                    ));
                }
            };

            let commit_term = match store_get_term(store, &commit_hex) {
                Ok(t) => t,
                Err(_) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/not-found",
                        format!("artifact not found: {commit_hex}"),
                        Some(op),
                    ));
                }
            };
            let commit = match gc_vcs::Commit::from_term(&commit_term) {
                Ok(c) => c,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-commit",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };

            for req in &class.required_obligations {
                if !commit.obligations.iter().any(|o| o == req) {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-obligation",
                        format!("commit missing required obligation: {req}"),
                        Some(op),
                    ));
                }
            }
            if !class.required_obligations.is_empty() && commit.evidence.is_empty() {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/missing-evidence",
                    "commit has required obligations but no evidence".to_string(),
                    Some(op),
                ));
            }
            let mut evidence_kinds: std::collections::BTreeSet<String> =
                std::collections::BTreeSet::new();
            for ev_h in &commit.evidence {
                let ev_term = match store_get_term(store, ev_h) {
                    Ok(t) => t,
                    Err(_) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/not-found",
                            format!("artifact not found: {ev_h}"),
                            Some(op),
                        ));
                    }
                };
                let ev = match gc_vcs::Evidence::from_term(&ev_term) {
                    Ok(ev) => ev,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-evidence",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                evidence_kinds.insert(gc_vcs::PolicyClass::normalize_evidence_kind(&ev.kind));
            }
            let missing_kinds =
                class.missing_required_evidence_kinds(&commit.obligations, &evidence_kinds);
            if !missing_kinds.is_empty() {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/missing-evidence-kind",
                    format!(
                        "commit evidence missing required kinds: {}",
                        missing_kinds.join(", ")
                    ),
                    Some(op),
                ));
            }

            if class.require_signatures {
                let signing_h = match gc_vcs::commit_signing_hash(&commit_term) {
                    Ok(h) => h,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-commit",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let mut valid: u64 = 0;
                let mut seen_pks: std::collections::BTreeSet<Vec<u8>> =
                    std::collections::BTreeSet::new();
                for at_h in &commit.attestations {
                    let at_term = match store_get_term(store, at_h) {
                        Ok(t) => t,
                        Err(_) => {
                            return Ok(mk_error(
                                error_tok,
                                "core/store/not-found",
                                format!("artifact not found: {at_h}"),
                                Some(op),
                            ));
                        }
                    };
                    let at = match gc_vcs::Attestation::from_term(&at_term) {
                        Ok(a) => a,
                        Err(e) => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-attestation",
                                e.to_string(),
                                Some(op),
                            ));
                        }
                    };
                    let pk_vec = at.pk.to_vec();
                    if seen_pks.contains(&pk_vec) {
                        continue;
                    }
                    if gc_vcs::verify_commit_attestation(
                        &at,
                        &signing_h,
                        &class.allowed_public_keys,
                    )
                    .is_ok()
                    {
                        seen_pks.insert(pk_vec);
                        valid = valid.saturating_add(1);
                    }
                }
                if valid < class.min_signatures {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-signatures",
                        format!(
                            "need {} valid signatures, got {valid}",
                            class.min_signatures
                        ),
                        Some(op),
                    ));
                }
            }

            let mut set_ref_mm = BTreeMap::new();
            set_ref_mm.insert(
                TermOrdKey(Term::symbol(":name")),
                Term::Str(refname.clone()),
            );
            set_ref_mm.insert(
                TermOrdKey(Term::symbol(":hash")),
                Term::Str(commit_hex.clone()),
            );
            set_ref_mm.insert(
                TermOrdKey(Term::symbol(":policy")),
                Term::Str(policy_h.clone()),
            );
            if let Some(eo) = &expected_old {
                set_ref_mm.insert(
                    TermOrdKey(Term::symbol(":expected-old")),
                    Term::Str(eo.clone()),
                );
            }

            let mut sync_payload_m = BTreeMap::new();
            sync_payload_m.insert(TermOrdKey(Term::symbol(":remote")), Term::Str(remote_s));
            sync_payload_m.insert(
                TermOrdKey(Term::symbol(":roots")),
                Term::Vector(vec![
                    Term::Str(commit_hex.clone()),
                    Term::Str(policy_h.clone()),
                ]),
            );
            if depth > 0 {
                sync_payload_m.insert(
                    TermOrdKey(Term::symbol(":depth")),
                    Term::Int((depth as i64).into()),
                );
            }
            sync_payload_m.insert(
                TermOrdKey(Term::symbol(":set-refs")),
                Term::Vector(vec![Term::Map(set_ref_mm)]),
            );

            let sync_payload = Term::Map(sync_payload_m);
            let sync_pol = pol.or_else(|| policy.op_policy("core/sync::push"));
            let sync_out = call_capability(
                "core/sync::push",
                &sync_payload,
                sync_pol,
                policy,
                Some(store),
                Some(refs),
                budget,
                error_tok,
            )?;

            let out = match sync_out {
                Value::Data(Term::Map(mut m)) => {
                    m.insert(TermOrdKey(Term::symbol(":commit")), Term::Str(commit_hex));
                    m.insert(TermOrdKey(Term::symbol(":ref")), Term::Str(refname));
                    m.insert(
                        TermOrdKey(Term::symbol(":provenance")),
                        commit_provenance_term(&commit),
                    );
                    Value::Data(Term::Map(m))
                }
                other => other,
            };
            Ok(out)
        }
        _ => unreachable!("dispatch_publish called with unsupported op: {op_eff}"),
    }
}
