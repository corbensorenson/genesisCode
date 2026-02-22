use std::collections::BTreeSet;

use gc_coreform::{Term, TermOrdKey, print_term};

use crate::schema::validate_hex_hash;

#[derive(Debug, Clone)]
pub struct RequirementsTraceGateContext<'a> {
    pub commit_hash: &'a str,
    pub snapshot_hash: &'a str,
    pub policy_hash: Option<&'a str>,
    pub commit_obligations: &'a [String],
    pub observed_evidence_kinds: &'a BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct ToolQualificationGateContext<'a> {
    pub commit_hash: &'a str,
    pub snapshot_hash: &'a str,
    pub policy_hash: Option<&'a str>,
}

pub fn validate_requirements_trace_evidence(
    evidence_term: &Term,
    ctx: &RequirementsTraceGateContext<'_>,
) -> Result<(), String> {
    let m = req_map(evidence_term, "requirements-trace")?;
    req_evidence_header(m, ":requirements-trace")?;
    let status = req_sym_or_str(m, ":status", "requirements-trace")?;
    if normalize_symbol_like(&status) != ":verified" {
        return Err(format!(
            "requirements-trace: :status must be :verified, got {status}"
        ));
    }
    let graph_h = req_str(m, ":graph-h", "requirements-trace")?;
    validate_hex_hash(&graph_h).map_err(|e| format!("requirements-trace: :graph-h: {e}"))?;
    let release = req_nested_map(m, ":release", "requirements-trace")?;
    if let Some(commit_h) = opt_str_or_nil(release, ":commit", "requirements-trace/:release")? {
        validate_hex_hash(&commit_h)
            .map_err(|e| format!("requirements-trace/:release :commit: {e}"))?;
        if commit_h != ctx.commit_hash {
            return Err(format!(
                "requirements-trace/:release :commit mismatch: expected {}, got {}",
                ctx.commit_hash, commit_h
            ));
        }
    }
    let snapshot_h = req_str(release, ":snapshot", "requirements-trace/:release")?;
    validate_hex_hash(&snapshot_h)
        .map_err(|e| format!("requirements-trace/:release :snapshot: {e}"))?;
    if snapshot_h != ctx.snapshot_hash {
        return Err(format!(
            "requirements-trace/:release :snapshot mismatch: expected {}, got {}",
            ctx.snapshot_hash, snapshot_h
        ));
    }
    if let Some(policy_h) = opt_str_or_nil(release, ":policy", "requirements-trace/:release")? {
        validate_hex_hash(&policy_h)
            .map_err(|e| format!("requirements-trace/:release :policy: {e}"))?;
        if let Some(expected) = ctx.policy_hash
            && policy_h != expected
        {
            return Err(format!(
                "requirements-trace/:release :policy mismatch: expected {}, got {}",
                expected, policy_h
            ));
        }
    }

    let reqs = req_vector(m, ":requirements", "requirements-trace")?;
    if reqs.is_empty() {
        return Err("requirements-trace: :requirements cannot be empty".to_string());
    }
    for (idx, req) in reqs.iter().enumerate() {
        let what = format!("requirements-trace/:requirements[{idx}]");
        let rm = req_map(req, &what)?;
        let id = req_str(rm, ":id", &what)?;
        if id.trim().is_empty() {
            return Err(format!("{what}: :id cannot be empty"));
        }
        let level = normalize_symbol_like(&req_sym_or_str(rm, ":level", &what)?);
        if level != ":system" && level != ":hlr" && level != ":llr" {
            return Err(format!(
                "{what}: :level must be one of :system|:hlr|:llr, got {level}"
            ));
        }
        // Optional parent/hazard links: schema-only checks.
        if let Some(xs) = opt_vector(rm, ":parents", &what)? {
            for (i, parent) in xs.iter().enumerate() {
                let p = req_str_term(parent, &format!("{what}:parents[{i}]"))?;
                if p.trim().is_empty() {
                    return Err(format!("{what}:parents[{i}] cannot be empty"));
                }
            }
        }
        if let Some(xs) = opt_vector(rm, ":hazards", &what)? {
            for (i, hazard) in xs.iter().enumerate() {
                let h = req_str_term(hazard, &format!("{what}:hazards[{i}]"))?;
                if h.trim().is_empty() {
                    return Err(format!("{what}:hazards[{i}] cannot be empty"));
                }
            }
        }
        let links = req_nested_map(rm, ":links", &what)?;
        let mut link_count = 0usize;
        if let Some(mods) = opt_vector(links, ":modules", &format!("{what}:links"))? {
            for (i, module) in mods.iter().enumerate() {
                let mm = req_map(module, &format!("{what}:links:modules[{i}]"))?;
                let path = req_str(mm, ":path", &format!("{what}:links:modules[{i}]"))?;
                if path.trim().is_empty() {
                    return Err(format!("{what}:links:modules[{i}] :path cannot be empty"));
                }
                let exports = req_vector(mm, ":exports", &format!("{what}:links:modules[{i}]"))?;
                if exports.is_empty() {
                    return Err(format!(
                        "{what}:links:modules[{i}] :exports cannot be empty"
                    ));
                }
                for (j, sym) in exports.iter().enumerate() {
                    req_sym_or_str_term(sym, &format!("{what}:links:modules[{i}]:exports[{j}]"))?;
                }
                link_count = link_count.saturating_add(1);
            }
        }
        if let Some(obs) = opt_vector(links, ":obligations", &format!("{what}:links"))? {
            for (i, ob) in obs.iter().enumerate() {
                let ob_name = req_sym_or_str_term(ob, &format!("{what}:links:obligations[{i}]"))?;
                if !ctx.commit_obligations.iter().any(|x| x == &ob_name) {
                    return Err(format!(
                        "{what}: obligation link is dangling (not present on commit): {ob_name}"
                    ));
                }
            }
            link_count = link_count.saturating_add(obs.len());
        }
        if let Some(kinds) = opt_vector(links, ":evidence-kinds", &format!("{what}:links"))? {
            for (i, kind_t) in kinds.iter().enumerate() {
                let kind = normalize_symbol_like(&req_sym_or_str_term(
                    kind_t,
                    &format!("{what}:links:evidence-kinds[{i}]"),
                )?);
                if !ctx.observed_evidence_kinds.contains(&kind) {
                    return Err(format!(
                        "{what}: evidence-kind link is dangling (not observed on commit): {kind}"
                    ));
                }
            }
            link_count = link_count.saturating_add(kinds.len());
        }
        if link_count == 0 {
            return Err(format!(
                "{what}: :links must include modules, obligations, or evidence-kinds"
            ));
        }
    }

    Ok(())
}

pub fn validate_tool_qualification_evidence(
    evidence_term: &Term,
    ctx: &ToolQualificationGateContext<'_>,
) -> Result<(), String> {
    let m = req_map(evidence_term, "tool-qualification")?;
    req_evidence_header(m, ":tool-qualification")?;
    let status = req_sym_or_str(m, ":status", "tool-qualification")?;
    if normalize_symbol_like(&status) != ":qualified" {
        return Err(format!(
            "tool-qualification: :status must be :qualified, got {status}"
        ));
    }

    let release = req_nested_map(m, ":release", "tool-qualification")?;
    if let Some(commit_h) = opt_str_or_nil(release, ":commit", "tool-qualification/:release")? {
        validate_hex_hash(&commit_h)
            .map_err(|e| format!("tool-qualification/:release :commit: {e}"))?;
        if commit_h != ctx.commit_hash {
            return Err(format!(
                "tool-qualification/:release :commit mismatch: expected {}, got {}",
                ctx.commit_hash, commit_h
            ));
        }
    }
    let snapshot_h = req_str(release, ":snapshot", "tool-qualification/:release")?;
    validate_hex_hash(&snapshot_h)
        .map_err(|e| format!("tool-qualification/:release :snapshot: {e}"))?;
    if snapshot_h != ctx.snapshot_hash {
        return Err(format!(
            "tool-qualification/:release :snapshot mismatch: expected {}, got {}",
            ctx.snapshot_hash, snapshot_h
        ));
    }
    if let Some(policy_h) = opt_str_or_nil(release, ":policy", "tool-qualification/:release")? {
        validate_hex_hash(&policy_h)
            .map_err(|e| format!("tool-qualification/:release :policy: {e}"))?;
        if let Some(expected) = ctx.policy_hash
            && policy_h != expected
        {
            return Err(format!(
                "tool-qualification/:release :policy mismatch: expected {}, got {}",
                expected, policy_h
            ));
        }
    }

    let requirements = req_vector(m, ":requirements", "tool-qualification")?;
    if requirements.is_empty() {
        return Err("tool-qualification: :requirements cannot be empty".to_string());
    }
    for (i, req) in requirements.iter().enumerate() {
        let rid = req_str_term(req, &format!("tool-qualification:requirements[{i}]"))?;
        if rid.trim().is_empty() {
            return Err(format!(
                "tool-qualification:requirements[{i}] cannot be empty"
            ));
        }
    }

    let tools = req_vector(m, ":tools", "tool-qualification")?;
    if tools.is_empty() {
        return Err("tool-qualification: :tools cannot be empty".to_string());
    }
    for (i, tool) in tools.iter().enumerate() {
        let tm = req_map(tool, &format!("tool-qualification:tools[{i}]"))?;
        let name = req_str(tm, ":name", &format!("tool-qualification:tools[{i}]"))?;
        if name.trim().is_empty() {
            return Err(format!(
                "tool-qualification:tools[{i}] :name cannot be empty"
            ));
        }
        let h = req_str(tm, ":blake3", &format!("tool-qualification:tools[{i}]"))?;
        validate_hex_hash(&h).map_err(|e| format!("tool-qualification:tools[{i}] :blake3: {e}"))?;
    }

    let tests = req_vector(m, ":qualification-tests", "tool-qualification")?;
    if tests.is_empty() {
        return Err("tool-qualification: :qualification-tests cannot be empty".to_string());
    }
    for (i, test) in tests.iter().enumerate() {
        let tm = req_map(
            test,
            &format!("tool-qualification:qualification-tests[{i}]"),
        )?;
        let id = req_str(
            tm,
            ":id",
            &format!("tool-qualification:qualification-tests[{i}]"),
        )?;
        if id.trim().is_empty() {
            return Err(format!(
                "tool-qualification:qualification-tests[{i}] :id cannot be empty"
            ));
        }
        let artifact = req_str(
            tm,
            ":artifact",
            &format!("tool-qualification:qualification-tests[{i}]"),
        )?;
        validate_hex_hash(&artifact)
            .map_err(|e| format!("tool-qualification:qualification-tests[{i}] :artifact: {e}"))?;
        let manifest = req_str(
            tm,
            ":manifest",
            &format!("tool-qualification:qualification-tests[{i}]"),
        )?;
        validate_hex_hash(&manifest)
            .map_err(|e| format!("tool-qualification:qualification-tests[{i}] :manifest: {e}"))?;
        let run_id = req_str(
            tm,
            ":run-id",
            &format!("tool-qualification:qualification-tests[{i}]"),
        )?;
        if run_id.trim().is_empty() {
            return Err(format!(
                "tool-qualification:qualification-tests[{i}] :run-id cannot be empty"
            ));
        }
        let runner = req_str(
            tm,
            ":runner",
            &format!("tool-qualification:qualification-tests[{i}]"),
        )?;
        if runner.trim().is_empty() {
            return Err(format!(
                "tool-qualification:qualification-tests[{i}] :runner cannot be empty"
            ));
        }
        let profile = req_str(
            tm,
            ":profile",
            &format!("tool-qualification:qualification-tests[{i}]"),
        )?;
        if profile.trim().is_empty() {
            return Err(format!(
                "tool-qualification:qualification-tests[{i}] :profile cannot be empty"
            ));
        }
        let test_snapshot = req_str(
            tm,
            ":snapshot",
            &format!("tool-qualification:qualification-tests[{i}]"),
        )?;
        validate_hex_hash(&test_snapshot)
            .map_err(|e| format!("tool-qualification:qualification-tests[{i}] :snapshot: {e}"))?;
        if test_snapshot != snapshot_h {
            return Err(format!(
                "tool-qualification:qualification-tests[{i}] :snapshot mismatch with release snapshot"
            ));
        }
        if let Some(policy_h) = opt_str_or_nil(
            tm,
            ":policy",
            &format!("tool-qualification:qualification-tests[{i}]"),
        )? {
            validate_hex_hash(&policy_h)
                .map_err(|e| format!("tool-qualification:qualification-tests[{i}] :policy: {e}"))?;
            if let Some(expected) = ctx.policy_hash
                && policy_h != expected
            {
                return Err(format!(
                    "tool-qualification:qualification-tests[{i}] :policy mismatch: expected {}, got {}",
                    expected, policy_h
                ));
            }
        }
        let result = normalize_symbol_like(&req_sym_or_str(
            tm,
            ":result",
            &format!("tool-qualification:qualification-tests[{i}]"),
        )?);
        if result != ":pass" {
            return Err(format!(
                "tool-qualification:qualification-tests[{i}] :result must be :pass, got {result}"
            ));
        }
    }

    Ok(())
}

fn req_map<'a>(
    t: &'a Term,
    what: &str,
) -> Result<&'a std::collections::BTreeMap<TermOrdKey, Term>, String> {
    match t {
        Term::Map(m) => Ok(m),
        _ => Err(format!("{what}: expected map, got {}", print_term(t))),
    }
}

fn req_evidence_header(
    m: &std::collections::BTreeMap<TermOrdKey, Term>,
    expected_kind: &str,
) -> Result<(), String> {
    let ty = req_sym_or_str(m, ":type", "evidence")?;
    if normalize_symbol_like(&ty) != ":vcs/evidence" {
        return Err(format!("evidence: wrong :type {ty}"));
    }
    let v = req_int(m, ":v", "evidence")?;
    if v != 1 {
        return Err(format!("evidence: unsupported :v {v}"));
    }
    let kind = normalize_symbol_like(&req_sym_or_str(m, ":kind", "evidence")?);
    if kind != expected_kind {
        return Err(format!(
            "evidence: wrong :kind {kind}, expected {expected_kind}"
        ));
    }
    Ok(())
}

fn req_int(
    m: &std::collections::BTreeMap<TermOrdKey, Term>,
    key: &str,
    what: &str,
) -> Result<i64, String> {
    match m.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Int(i)) => {
            use num_traits::ToPrimitive;
            i.to_i64()
                .ok_or_else(|| format!("{what}: {key} out of i64 range"))
        }
        Some(other) => Err(format!(
            "{what}: {key} must be int, got {}",
            print_term(other)
        )),
        None => Err(format!("{what}: missing {key}")),
    }
}

fn req_str(
    m: &std::collections::BTreeMap<TermOrdKey, Term>,
    key: &str,
    what: &str,
) -> Result<String, String> {
    req_str_term(
        m.get(&TermOrdKey(Term::symbol(key)))
            .ok_or_else(|| format!("{what}: missing {key}"))?,
        &format!("{what}:{key}"),
    )
}

fn req_str_term(t: &Term, what: &str) -> Result<String, String> {
    match t {
        Term::Str(s) => Ok(s.clone()),
        _ => Err(format!("{what}: expected string, got {}", print_term(t))),
    }
}

fn req_sym_or_str(
    m: &std::collections::BTreeMap<TermOrdKey, Term>,
    key: &str,
    what: &str,
) -> Result<String, String> {
    req_sym_or_str_term(
        m.get(&TermOrdKey(Term::symbol(key)))
            .ok_or_else(|| format!("{what}: missing {key}"))?,
        &format!("{what}:{key}"),
    )
}

fn req_sym_or_str_term(t: &Term, what: &str) -> Result<String, String> {
    match t {
        Term::Symbol(s) | Term::Str(s) => Ok(s.clone()),
        _ => Err(format!(
            "{what}: expected symbol or string, got {}",
            print_term(t)
        )),
    }
}

fn req_vector<'a>(
    m: &'a std::collections::BTreeMap<TermOrdKey, Term>,
    key: &str,
    what: &str,
) -> Result<&'a Vec<Term>, String> {
    match m.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Vector(xs)) => Ok(xs),
        Some(other) => Err(format!(
            "{what}: {key} must be vector, got {}",
            print_term(other)
        )),
        None => Err(format!("{what}: missing {key}")),
    }
}

fn opt_vector<'a>(
    m: &'a std::collections::BTreeMap<TermOrdKey, Term>,
    key: &str,
    what: &str,
) -> Result<Option<&'a Vec<Term>>, String> {
    match m.get(&TermOrdKey(Term::symbol(key))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Vector(xs)) => Ok(Some(xs)),
        Some(other) => Err(format!(
            "{what}: {key} must be vector or nil, got {}",
            print_term(other)
        )),
    }
}

fn req_nested_map<'a>(
    m: &'a std::collections::BTreeMap<TermOrdKey, Term>,
    key: &str,
    what: &str,
) -> Result<&'a std::collections::BTreeMap<TermOrdKey, Term>, String> {
    match m.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Map(mm)) => Ok(mm),
        Some(other) => Err(format!(
            "{what}: {key} must be map, got {}",
            print_term(other)
        )),
        None => Err(format!("{what}: missing {key}")),
    }
}

fn opt_str_or_nil(
    m: &std::collections::BTreeMap<TermOrdKey, Term>,
    key: &str,
    what: &str,
) -> Result<Option<String>, String> {
    match m.get(&TermOrdKey(Term::symbol(key))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Str(s)) => Ok(Some(s.clone())),
        Some(other) => Err(format!(
            "{what}: {key} must be string or nil, got {}",
            print_term(other)
        )),
    }
}

fn normalize_symbol_like(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with(':') {
        trimmed.to_string()
    } else {
        format!(":{trimmed}")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RequirementsTraceGateContext, ToolQualificationGateContext, normalize_symbol_like,
        validate_requirements_trace_evidence, validate_tool_qualification_evidence,
    };
    use gc_coreform::parse_term;
    use std::collections::BTreeSet;

    #[test]
    fn requirements_trace_validator_rejects_unverified_status() {
        let t = parse_term(
            r#"
            {
              :type :vcs/evidence
              :v 1
              :kind :requirements-trace
              :status :pending
              :graph-h "0000000000000000000000000000000000000000000000000000000000000000"
              :release {:commit "1111111111111111111111111111111111111111111111111111111111111111"
                        :snapshot "2222222222222222222222222222222222222222222222222222222222222222"
                        :policy nil}
              :requirements []
            }
            "#,
        )
        .expect("term");
        let observed = BTreeSet::new();
        let ctx = RequirementsTraceGateContext {
            commit_hash: "1111111111111111111111111111111111111111111111111111111111111111",
            snapshot_hash: "2222222222222222222222222222222222222222222222222222222222222222",
            policy_hash: None,
            commit_obligations: &[],
            observed_evidence_kinds: &observed,
        };
        let err = validate_requirements_trace_evidence(&t, &ctx).unwrap_err();
        assert!(err.contains(":status"), "{err}");
    }

    #[test]
    fn tool_qualification_validator_accepts_minimal_valid_shape() {
        let t = parse_term(
            r#"
            {
              :type :vcs/evidence
              :v 1
              :kind :tool-qualification
              :status :qualified
              :release {:commit "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        :snapshot "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
                        :policy nil}
              :requirements ["TQ-1"]
              :tools [{:name "genesis"
                       :path "./bin/genesis"
                       :blake3 "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                       :size-bytes 1}]
              :qualification-tests [{:id "selfhost-boundary"
                                     :artifact "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                                     :manifest "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
                                     :run-id "run-1"
                                     :runner "gcpm-assurance"
                                     :profile "dal-a"
                                     :snapshot "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
                                     :policy nil
                                     :result :pass}]
            }
            "#,
        )
        .expect("term");
        let ctx = ToolQualificationGateContext {
            commit_hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            snapshot_hash: "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
            policy_hash: None,
        };
        validate_tool_qualification_evidence(&t, &ctx).expect("valid qualification");
        assert_eq!(normalize_symbol_like("verified"), ":verified");
    }

    #[test]
    fn requirements_trace_validator_allows_precommit_release_binding() {
        let t = parse_term(
            r#"
            {
              :type :vcs/evidence
              :v 1
              :kind :requirements-trace
              :status :verified
              :graph-h "0000000000000000000000000000000000000000000000000000000000000000"
              :release {:commit nil
                        :snapshot "2222222222222222222222222222222222222222222222222222222222222222"
                        :policy nil}
              :requirements [{
                :id "SYS-1"
                :level :system
                :parents []
                :hazards []
                :links {:evidence-kinds [:requirements-trace]}
              }]
            }
            "#,
        )
        .expect("term");
        let observed = BTreeSet::from([":requirements-trace".to_string()]);
        let ctx = RequirementsTraceGateContext {
            commit_hash: "1111111111111111111111111111111111111111111111111111111111111111",
            snapshot_hash: "2222222222222222222222222222222222222222222222222222222222222222",
            policy_hash: None,
            commit_obligations: &[],
            observed_evidence_kinds: &observed,
        };
        validate_requirements_trace_evidence(&t, &ctx).expect("valid precommit trace");
    }

    #[test]
    fn tool_qualification_validator_allows_precommit_release_binding() {
        let t = parse_term(
            r#"
            {
              :type :vcs/evidence
              :v 1
              :kind :tool-qualification
              :status :qualified
              :release {:commit nil
                        :snapshot "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
                        :policy nil}
              :requirements ["TQ-1"]
              :tools [{:name "genesis"
                       :path "./bin/genesis"
                       :blake3 "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                       :size-bytes 1}]
              :qualification-tests [{:id "selfhost-boundary"
                                     :artifact "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                                     :manifest "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
                                     :run-id "run-1"
                                     :runner "gcpm-assurance"
                                     :profile "dal-a"
                                     :snapshot "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
                                     :policy nil
                                     :result :pass}]
            }
            "#,
        )
        .expect("term");
        let ctx = ToolQualificationGateContext {
            commit_hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            snapshot_hash: "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
            policy_hash: None,
        };
        validate_tool_qualification_evidence(&t, &ctx).expect("valid precommit qualification");
    }
}
