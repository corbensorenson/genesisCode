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
            let mut requirements_trace_terms: Vec<Term> = Vec::new();
            let mut tool_qualification_terms: Vec<Term> = Vec::new();
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
                let norm_kind = gc_vcs::PolicyClass::normalize_evidence_kind(&ev.kind);
                if norm_kind == ":requirements-trace" {
                    requirements_trace_terms.push(ev_term.clone());
                } else if norm_kind == ":tool-qualification" {
                    tool_qualification_terms.push(ev_term.clone());
                }
                evidence_kinds.insert(norm_kind);
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
            let required_kinds = class.required_evidence_kind_set(&commit.obligations);
            if required_kinds.contains(":requirements-trace") {
                if requirements_trace_terms.is_empty() {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-requirements-trace",
                        "required evidence kind :requirements-trace is present but no trace artifact was parsed".to_string(),
                        Some(op),
                    ));
                }
                let ctx = gc_vcs::RequirementsTraceGateContext {
                    commit_hash: &commit_hex,
                    snapshot_hash: &commit.result,
                    policy_hash: Some(&policy_h),
                    commit_obligations: &commit.obligations,
                    observed_evidence_kinds: &evidence_kinds,
                };
                for t in &requirements_trace_terms {
                    if let Err(e) = gc_vcs::validate_requirements_trace_evidence(t, &ctx) {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/invalid-requirements-trace",
                            e,
                            Some(op),
                        ));
                    }
                }
            }
            if required_kinds.contains(":tool-qualification") {
                if tool_qualification_terms.is_empty() {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-tool-qualification",
                        "required evidence kind :tool-qualification is present but no qualification artifact was parsed".to_string(),
                        Some(op),
                    ));
                }
                let ctx = gc_vcs::ToolQualificationGateContext {
                    commit_hash: &commit_hex,
                    snapshot_hash: &commit.result,
                    policy_hash: Some(&policy_h),
                };
                for t in &tool_qualification_terms {
                    if let Err(e) = gc_vcs::validate_tool_qualification_evidence(t, &ctx) {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/invalid-tool-qualification",
                            e,
                            Some(op),
                        ));
                    }
                }
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
                let mut role_signers: std::collections::BTreeMap<
                    String,
                    std::collections::BTreeSet<Vec<u8>>,
                > = std::collections::BTreeMap::new();
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
                    if gc_vcs::verify_commit_attestation(
                        &at,
                        &signing_h,
                        &class.allowed_public_keys,
                    )
                    .is_ok()
                    {
                        if seen_pks.insert(pk_vec.clone()) {
                            valid = valid.saturating_add(1);
                        }
                        if let Some(role) = at.role.as_deref() {
                            let norm = gc_vcs::PolicyClass::normalize_attestation_role(role);
                            if norm != ":" {
                                role_signers.entry(norm).or_default().insert(pk_vec);
                            }
                        }
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
                for role in &class.required_attestation_roles {
                    let count = role_signers.get(role).map(|s| s.len()).unwrap_or(0);
                    if count == 0 {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/missing-attestation-role",
                            format!("missing required attestation role {role}"),
                            Some(op),
                        ));
                    }
                }
                for (role, min) in &class.role_min_signatures {
                    let count = role_signers.get(role).map(|s| s.len()).unwrap_or(0);
                    if count < *min as usize {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/missing-attestation-role-signatures",
                            format!("role {role} requires {min} distinct signer(s), got {count}"),
                            Some(op),
                        ));
                    }
                }
                for (left, right) in &class.independent_role_pairs {
                    let left_set = role_signers.get(left);
                    let right_set = role_signers.get(right);
                    if left_set.map_or(0, |s| s.len()) == 0 || right_set.map_or(0, |s| s.len()) == 0
                    {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/missing-attestation-role",
                            format!(
                                "independence pair requires both roles present: {left}, {right}"
                            ),
                            Some(op),
                        ));
                    }
                    if let (Some(a), Some(b)) = (left_set, right_set)
                        && a.iter().any(|pk| b.contains(pk))
                    {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/role-independence-violation",
                            format!("roles {left} and {right} must be signed by independent keys"),
                            Some(op),
                        ));
                    }
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
                Value::Data(t) => match t.as_ref() {
                    Term::Map(m) => {
                        let mut m = m.clone();
                        m.insert(TermOrdKey(Term::symbol(":commit")), Term::Str(commit_hex));
                        m.insert(TermOrdKey(Term::symbol(":ref")), Term::Str(refname));
                        m.insert(
                            TermOrdKey(Term::symbol(":provenance")),
                            commit_provenance_term(&commit),
                        );
                        Value::data(Term::Map(m))
                    }
                    _ => Value::Data(t),
                },
                other => other,
            };
            Ok(out)
        }
        "core/pkg-low::bridge" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg-low::bridge".to_string())
            })?;
            let ecosystem = match payload_pkg_bridge_ecosystem(payload) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op)));
                }
            };
            let name = match payload_pkg_name(payload) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op)));
                }
            };
            let version = match payload_pkg_bridge_version(payload) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op)));
                }
            };
            let source = match payload_pkg_bridge_source(payload) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op)));
                }
            };
            let source_hash = match payload_pkg_bridge_source_hash(payload) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op)));
                }
            };
            let key_id = match payload_pkg_bridge_key_id(payload) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op)));
                }
            };
            let public_key_hex = match payload_pkg_bridge_public_key(payload) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op)));
                }
            };
            let lock_path = match payload_pkg_bridge_lock(payload) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op)));
                }
            };
            let dep_name = match payload_pkg_bridge_dep_name(payload) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op)));
                }
            };
            let registry_alias = payload_pkg_registry(payload);

            if lock_path.is_some() && dep_name.is_none() {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/bad-payload",
                    "bridge lock updates require :dep-name when :lock is provided".to_string(),
                    Some(op),
                ));
            }
            if dep_name.is_some() && lock_path.is_none() {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/bad-payload",
                    "bridge :dep-name requires :lock".to_string(),
                    Some(op),
                ));
            }

            let provenance_term = Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":type")),
                        Term::symbol(":gcpm/external-provenance"),
                    ),
                    (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
                    (
                        TermOrdKey(Term::symbol(":ecosystem")),
                        Term::Str(ecosystem.clone()),
                    ),
                    (TermOrdKey(Term::symbol(":name")), Term::Str(name.clone())),
                    (
                        TermOrdKey(Term::symbol(":version")),
                        Term::Str(version.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":source")),
                        Term::Str(source.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":source-hash")),
                        Term::Str(source_hash.clone()),
                    ),
                ]
                .into_iter()
                .collect(),
            );
            let provenance_root = match store_put_with_budget(
                store,
                print_term(&provenance_term).as_bytes(),
                policy,
                budget,
                error_tok,
                op,
            ) {
                Ok(h) => h,
                Err(v) => return Ok(v),
            };

            let conversion_data_term = Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":type")),
                        Term::symbol(":gcpm/bridge-conversion"),
                    ),
                    (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
                    (
                        TermOrdKey(Term::symbol(":toolchain")),
                        Term::Str("genesis/pkg-low::bridge-v0.1".to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":ecosystem")),
                        Term::Str(ecosystem.clone()),
                    ),
                    (TermOrdKey(Term::symbol(":name")), Term::Str(name.clone())),
                    (
                        TermOrdKey(Term::symbol(":version")),
                        Term::Str(version.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":source")),
                        Term::Str(source.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":source-hash")),
                        Term::Str(source_hash.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":provenance-root")),
                        Term::Str(provenance_root.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":replay")),
                        Term::Map(
                            [
                                (
                                    TermOrdKey(Term::symbol(":algorithm")),
                                    Term::Str("identity-source-hash".to_string()),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":input-hash")),
                                    Term::Str(source_hash.clone()),
                                ),
                            ]
                            .into_iter()
                            .collect(),
                        ),
                    ),
                ]
                .into_iter()
                .collect(),
            );
            let conversion_data_h = match store_put_with_budget(
                store,
                print_term(&conversion_data_term).as_bytes(),
                policy,
                budget,
                error_tok,
                op,
            ) {
                Ok(h) => h,
                Err(v) => return Ok(v),
            };

            let conversion_evidence_term = Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":type")),
                        Term::symbol(":vcs/evidence"),
                    ),
                    (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
                    (
                        TermOrdKey(Term::symbol(":kind")),
                        Term::symbol(":equivalence"),
                    ),
                    (
                        TermOrdKey(Term::symbol(":inputs")),
                        Term::Vector(vec![Term::Str(provenance_root.clone())]),
                    ),
                    (
                        TermOrdKey(Term::symbol(":outputs")),
                        Term::Vector(Vec::new()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":data")),
                        Term::Str(conversion_data_h.clone()),
                    ),
                ]
                .into_iter()
                .collect(),
            );
            let conversion_evidence = match store_put_with_budget(
                store,
                print_term(&conversion_evidence_term).as_bytes(),
                policy,
                budget,
                error_tok,
                op,
            ) {
                Ok(h) => h,
                Err(v) => return Ok(v),
            };

            let patch_term = Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":type")),
                        Term::symbol(":vcs/patch"),
                    ),
                    (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
                    (TermOrdKey(Term::symbol(":ops")), Term::Vector(Vec::new())),
                ]
                .into_iter()
                .collect(),
            );
            let patch_h = match store_put_with_budget(
                store,
                print_term(&patch_term).as_bytes(),
                policy,
                budget,
                error_tok,
                op,
            ) {
                Ok(h) => h,
                Err(v) => return Ok(v),
            };

            let obligations = [
                "core/obligation::external-provenance".to_string(),
                "core/obligation::replayable-conversion".to_string(),
            ];
            let snapshot_term = Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":type")),
                        Term::symbol(":vcs/snapshot"),
                    ),
                    (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
                    (TermOrdKey(Term::symbol(":kind")), Term::symbol(":package")),
                    (
                        TermOrdKey(Term::symbol(":pkg/name")),
                        Term::Str(name.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":pkg/version")),
                        Term::Str(version.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":modules")),
                        Term::Vector(Vec::new()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":obligations")),
                        Term::Vector(obligations.iter().cloned().map(Term::Symbol).collect()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":meta")),
                        Term::Map(
                            [
                                (
                                    TermOrdKey(Term::symbol(":bridge/ecosystem")),
                                    Term::Str(ecosystem.clone()),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":bridge/source")),
                                    Term::Str(source.clone()),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":bridge/source-hash")),
                                    Term::Str(source_hash.clone()),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":bridge/provenance-root")),
                                    Term::Str(provenance_root.clone()),
                                ),
                            ]
                            .into_iter()
                            .collect(),
                        ),
                    ),
                ]
                .into_iter()
                .collect(),
            );
            let snapshot_h = match store_put_with_budget(
                store,
                print_term(&snapshot_term).as_bytes(),
                policy,
                budget,
                error_tok,
                op,
            ) {
                Ok(h) => h,
                Err(v) => return Ok(v),
            };

            let commit_term_without_attestation = Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":type")),
                        Term::symbol(":vcs/commit"),
                    ),
                    (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
                    (
                        TermOrdKey(Term::symbol(":parents")),
                        Term::Vector(Vec::new()),
                    ),
                    (TermOrdKey(Term::symbol(":base")), Term::Nil),
                    (
                        TermOrdKey(Term::symbol(":patch")),
                        Term::Str(patch_h.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":result")),
                        Term::Str(snapshot_h.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":obligations")),
                        Term::Vector(obligations.iter().cloned().map(Term::Str).collect()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":evidence")),
                        Term::Vector(vec![Term::Str(conversion_evidence.clone())]),
                    ),
                    (
                        TermOrdKey(Term::symbol(":attestations")),
                        Term::Vector(Vec::new()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":message")),
                        Term::Str(format!(
                            "bridge {} {} {}",
                            ecosystem.as_str(),
                            name.as_str(),
                            version.as_str()
                        )),
                    ),
                ]
                .into_iter()
                .collect(),
            );
            let signing_h = match gc_vcs::commit_signing_hash(&commit_term_without_attestation) {
                Ok(h) => h,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bridge-signing-hash",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let public_key = match gc_vcs::hex_to_bytes32(&public_key_hex) {
                Ok(bytes) => bytes,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        format!(":public-key: {e}"),
                        Some(op),
                    ));
                }
            };

            let sign_payload = Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":algorithm")),
                        Term::Str("ed25519".to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":key-id")),
                        Term::Str(key_id.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":message")),
                        Term::Bytes(signing_h.to_vec().into()),
                    ),
                ]
                .into_iter()
                .collect(),
            );
            let sign_pol = policy.op_policy("core/crypto::sign").or(pol);
            let sign_out = call_capability(
                "core/crypto::sign",
                &sign_payload,
                sign_pol,
                policy,
                Some(store),
                refs,
                budget,
                error_tok,
            )?;
            let signature = match sign_out {
                Value::Sealed { .. } => return Ok(sign_out),
                Value::Data(t) => {
                    let Term::Map(mm) = t.as_ref() else {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bridge-signature",
                            "core/crypto::sign response must be a map".to_string(),
                            Some(op),
                        ));
                    };
                    match mm.get(&TermOrdKey(Term::symbol(":signature"))) {
                        Some(Term::Bytes(sig)) if sig.len() == 64 => sig.to_vec(),
                        Some(Term::Bytes(sig)) => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bridge-signature",
                                format!("signature must be 64 bytes, got {}", sig.len()),
                                Some(op),
                            ));
                        }
                        Some(other) => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bridge-signature",
                                format!("signature must be bytes, got {}", print_term(other)),
                                Some(op),
                            ));
                        }
                        None => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bridge-signature",
                                "core/crypto::sign response missing :signature".to_string(),
                                Some(op),
                            ));
                        }
                    }
                }
                other => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bridge-signature",
                        format!(
                            "unexpected core/crypto::sign response: {}",
                            other.debug_repr()
                        ),
                        Some(op),
                    ));
                }
            };

            let attestation_term = Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":type")),
                        Term::symbol(":vcs/attestation"),
                    ),
                    (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
                    (
                        TermOrdKey(Term::symbol(":alg")),
                        Term::Str("ed25519".to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":signing-h")),
                        Term::Bytes(signing_h.to_vec().into()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":pk")),
                        Term::Bytes(public_key.to_vec().into()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":sig")),
                        Term::Bytes(signature.into()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":role")),
                        Term::Symbol(":mirror-converter".to_string()),
                    ),
                ]
                .into_iter()
                .collect(),
            );
            let attestation_h = match store_put_with_budget(
                store,
                print_term(&attestation_term).as_bytes(),
                policy,
                budget,
                error_tok,
                op,
            ) {
                Ok(h) => h,
                Err(v) => return Ok(v),
            };

            let commit_term = Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":type")),
                        Term::symbol(":vcs/commit"),
                    ),
                    (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
                    (
                        TermOrdKey(Term::symbol(":parents")),
                        Term::Vector(Vec::new()),
                    ),
                    (TermOrdKey(Term::symbol(":base")), Term::Nil),
                    (
                        TermOrdKey(Term::symbol(":patch")),
                        Term::Str(patch_h.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":result")),
                        Term::Str(snapshot_h.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":obligations")),
                        Term::Vector(obligations.iter().cloned().map(Term::Str).collect()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":evidence")),
                        Term::Vector(vec![Term::Str(conversion_evidence.clone())]),
                    ),
                    (
                        TermOrdKey(Term::symbol(":attestations")),
                        Term::Vector(vec![Term::Str(attestation_h.clone())]),
                    ),
                    (
                        TermOrdKey(Term::symbol(":message")),
                        Term::Str(format!(
                            "bridge {} {} {}",
                            ecosystem.as_str(),
                            name.as_str(),
                            version.as_str()
                        )),
                    ),
                ]
                .into_iter()
                .collect(),
            );
            let commit_h = match store_put_with_budget(
                store,
                print_term(&commit_term).as_bytes(),
                policy,
                budget,
                error_tok,
                op,
            ) {
                Ok(h) => h,
                Err(v) => return Ok(v),
            };

            let mut lock_h: Option<String> = None;
            if let Some(lock_s) = lock_path {
                let Some(dep) = dep_name.clone() else {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        "bridge lock updates require :dep-name when :lock is provided".to_string(),
                        Some(op),
                    ));
                };
                let base_dir = effective_base_dir(pol)?;
                let lock_path = match sandbox_path_read(&base_dir, &lock_s) {
                    Ok(p) => p,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/missing-lock",
                            format!("{e}"),
                            Some(op),
                        ));
                    }
                };
                let mut lock = match gc_pkg::GenesisLock::load(&lock_path) {
                    Ok(l) => l,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-lock",
                            format!("{e}"),
                            Some(op),
                        ));
                    }
                };
                lock.set_requirement_with_metadata(
                    &dep,
                    &format!("commit:{commit_h}"),
                    gc_pkg::UpdatePolicy::Manual,
                    registry_alias.clone(),
                    Some(gc_pkg::ResolutionStrategy::Pinned),
                    None,
                );
                lock.locked.insert(
                    dep.clone(),
                    gc_pkg::LockedEntry {
                        commit: Some(commit_h.clone()),
                        snapshot: snapshot_h.clone(),
                        registry: registry_alias.clone(),
                        source_selector: format!("commit:{commit_h}"),
                        resolved_ref: None,
                        exports_hash: None,
                        environment_fingerprint: None,
                    },
                );
                let dep_key_fragment: String = dep
                    .chars()
                    .map(|ch| {
                        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                            ch
                        } else {
                            '_'
                        }
                    })
                    .collect();
                let dep_key_hash = blake3::hash(dep.as_bytes()).to_hex().to_string();
                let dep_key = format!("{dep_key_fragment}_{}", &dep_key_hash[..8]);
                lock.artifacts.insert(
                    format!("bridge_{dep_key}_provenance_root"),
                    provenance_root.clone(),
                );
                lock.artifacts.insert(
                    format!("bridge_{dep_key}_conversion_evidence"),
                    conversion_evidence.clone(),
                );
                lock.artifacts.insert(
                    format!("bridge_{dep_key}_attestation"),
                    attestation_h.clone(),
                );
                lock.artifacts
                    .insert(format!("bridge_{dep_key}_commit"), commit_h.clone());
                let lock_bytes = lock.to_toml_canonical();
                let lock_hash = blake3::hash(lock_bytes.as_bytes()).to_hex().to_string();
                let lock_write_path = match sandbox_path_write(&base_dir, &lock_s, false) {
                    Ok(p) => p,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/caps/path-escape",
                            format!("{e}"),
                            Some(op),
                        ));
                    }
                };
                if let Err(e) = atomic_write_text(&lock_write_path, lock_bytes.as_bytes()) {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/io-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
                lock_h = Some(lock_hash);
            }

            let mut out = BTreeMap::new();
            out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            out.insert(TermOrdKey(Term::symbol(":ecosystem")), Term::Str(ecosystem));
            out.insert(TermOrdKey(Term::symbol(":name")), Term::Str(name));
            out.insert(TermOrdKey(Term::symbol(":version")), Term::Str(version));
            out.insert(TermOrdKey(Term::symbol(":source")), Term::Str(source));
            out.insert(
                TermOrdKey(Term::symbol(":source-hash")),
                Term::Str(source_hash),
            );
            out.insert(
                TermOrdKey(Term::symbol(":provenance-root")),
                Term::Str(provenance_root),
            );
            out.insert(
                TermOrdKey(Term::symbol(":conversion-evidence")),
                Term::Str(conversion_evidence),
            );
            out.insert(TermOrdKey(Term::symbol(":snapshot")), Term::Str(snapshot_h));
            out.insert(TermOrdKey(Term::symbol(":patch")), Term::Str(patch_h));
            out.insert(
                TermOrdKey(Term::symbol(":attestation")),
                Term::Str(attestation_h),
            );
            out.insert(TermOrdKey(Term::symbol(":commit")), Term::Str(commit_h));
            out.insert(
                TermOrdKey(Term::symbol(":dep-name")),
                dep_name.map(Term::Str).unwrap_or(Term::Nil),
            );
            out.insert(
                TermOrdKey(Term::symbol(":registry")),
                registry_alias.map(Term::Str).unwrap_or(Term::Nil),
            );
            out.insert(
                TermOrdKey(Term::symbol(":lock-h")),
                lock_h.map(Term::Str).unwrap_or(Term::Nil),
            );
            Ok(Value::data(Term::Map(out)))
        }
        _ => Ok(mk_error(
            error_tok,
            "core/caps/unknown-op-eff",
            format!("core/pkg-low dispatch received unsupported op_eff: {op_eff}"),
            Some(op),
        )),
    }
}
