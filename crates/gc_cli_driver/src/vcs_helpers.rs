use super::*;

pub(crate) fn extract_refs_get_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":hash"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        Some(Term::Nil) => Some("nil".to_string()),
        _ => None,
    }
}

pub(crate) fn extract_refs_set_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":hash"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        Some(Term::Nil) => Some("nil".to_string()),
        _ => None,
    }
}

pub(crate) fn extract_refs_list_pairs(v: &Value) -> Option<Vec<(String, String)>> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    let Term::Vector(xs) = m.get(&gc_coreform::TermOrdKey(Term::symbol(":refs")))? else {
        return None;
    };
    let mut out = Vec::new();
    for x in xs {
        let Term::Map(em) = x else { return None };
        let name = match em.get(&gc_coreform::TermOrdKey(Term::symbol(":name"))) {
            Some(Term::Str(s)) => s.clone(),
            _ => return None,
        };
        let hash = match em.get(&gc_coreform::TermOrdKey(Term::symbol(":hash"))) {
            Some(Term::Str(s)) => s.clone(),
            Some(Term::Nil) => "nil".to_string(),
            _ => return None,
        };
        out.push((name, hash));
    }
    Some(out)
}

pub(crate) fn parse_pkg_spec(spec: &str) -> Result<(String, String), String> {
    let (name, sel) = spec
        .split_once('@')
        .ok_or_else(|| "spec must be <name>@<selector>".to_string())?;
    let name = name.trim();
    let sel = sel.trim();
    if name.is_empty() || sel.is_empty() {
        return Err("spec must be <name>@<selector> (both non-empty)".to_string());
    }
    Ok((name.to_string(), sel.to_string()))
}

pub(crate) fn normalize_pkg_add_strategy(
    selector: &str,
    strategy: Option<&str>,
    tag_policy: Option<&str>,
) -> Result<(Option<String>, Option<String>), CliError> {
    let strategy = match strategy {
        Some(raw) => raw.parse::<gc_pkg::ResolutionStrategy>().map_err(|_| {
            cli_err(
                EX_PARSE,
                "pkg/spec",
                format!("invalid --strategy `{raw}` (expected pinned|track-ref|tag-policy)"),
            )
        })?,
        None => gc_pkg::infer_strategy(selector),
    };

    if matches!(strategy, gc_pkg::ResolutionStrategy::TagPolicy)
        && !matches!(
            gc_pkg::classify_selector(selector),
            Some(gc_pkg::SelectorKind::TagRef)
        )
    {
        return Err(cli_err(
            EX_PARSE,
            "pkg/spec",
            "tag-policy strategy requires selector under refs/tags/*".to_string(),
        ));
    }
    if !matches!(strategy, gc_pkg::ResolutionStrategy::TagPolicy) && tag_policy.is_some() {
        return Err(cli_err(
            EX_PARSE,
            "pkg/spec",
            "--tag-policy can only be used with --strategy tag-policy".to_string(),
        ));
    }

    let strategy_s = Some(strategy.as_str().to_string());
    let tag_policy_s = if matches!(strategy, gc_pkg::ResolutionStrategy::TagPolicy) {
        Some(tag_policy.unwrap_or("exact").to_string())
    } else {
        None
    };
    Ok((strategy_s, tag_policy_s))
}

#[derive(Debug, Clone)]
pub(crate) struct SetRefSpec {
    pub(crate) name: String,
    pub(crate) hash: String,
    pub(crate) policy: String,
    pub(crate) expected_old: Option<String>,
}

pub(crate) fn parse_set_ref_spec(spec: &str) -> Result<SetRefSpec, CliError> {
    let (base, expected_old_raw) = match spec.split_once('@') {
        None => (spec, None),
        Some((lhs, rhs)) => (lhs, Some(rhs)),
    };

    let mut it = base.rsplitn(3, ':');
    let Some(policy) = it.next() else {
        return Err(cli_err(
            EX_PARSE,
            "sync/set-ref",
            "set-ref must be <refname>:<commit-hash>:<policy-hash>[@<expected-old-hash|nil>]"
                .to_string(),
        ));
    };
    let Some(hash) = it.next() else {
        return Err(cli_err(
            EX_PARSE,
            "sync/set-ref",
            "set-ref must be <refname>:<commit-hash>:<policy-hash>[@<expected-old-hash|nil>]"
                .to_string(),
        ));
    };
    let Some(name) = it.next() else {
        return Err(cli_err(
            EX_PARSE,
            "sync/set-ref",
            "set-ref must be <refname>:<commit-hash>:<policy-hash>[@<expected-old-hash|nil>]"
                .to_string(),
        ));
    };
    let name = name.trim();
    let hash = hash.trim();
    let policy = policy.trim();

    if name.is_empty() || hash.is_empty() || policy.is_empty() {
        return Err(cli_err(
            EX_PARSE,
            "sync/set-ref",
            "set-ref fields must be non-empty".to_string(),
        ));
    }
    if !is_hex64(hash) {
        return Err(cli_err(
            EX_PARSE,
            "sync/set-ref",
            "set-ref commit hash must be 64-hex".to_string(),
        ));
    }
    if !is_hex64(policy) {
        return Err(cli_err(
            EX_PARSE,
            "sync/set-ref",
            "set-ref policy hash must be 64-hex".to_string(),
        ));
    }
    let expected_old = match expected_old_raw.map(str::trim) {
        None => None,
        Some("") => {
            return Err(cli_err(
                EX_PARSE,
                "sync/set-ref",
                "set-ref expected-old must be non-empty when provided".to_string(),
            ));
        }
        Some(s) => {
            if s != "nil" && !is_hex64(s) {
                return Err(cli_err(
                    EX_PARSE,
                    "sync/set-ref",
                    "set-ref expected-old must be 64-hex or `nil`".to_string(),
                ));
            }
            Some(if s == "nil" {
                "nil".to_string()
            } else {
                s.to_ascii_lowercase()
            })
        }
    };

    Ok(SetRefSpec {
        name: name.to_string(),
        hash: hash.to_ascii_lowercase(),
        policy: policy.to_ascii_lowercase(),
        expected_old,
    })
}

pub(crate) fn parse_sync_set_refs(specs: &[String]) -> Result<Vec<SetRefSpec>, CliError> {
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut out = Vec::with_capacity(specs.len());
    for spec in specs {
        let parsed = parse_set_ref_spec(spec)?;
        if !seen.insert(parsed.name.clone()) {
            return Err(cli_err(
                EX_PARSE,
                "sync/set-ref",
                format!("duplicate set-ref target: {}", parsed.name),
            ));
        }
        out.push(parsed);
    }
    Ok(out)
}

pub(crate) fn is_hex64(s: &str) -> bool {
    if s.len() != 64 {
        return false;
    }
    s.as_bytes().iter().all(|b| b.is_ascii_hexdigit())
}

pub(crate) fn parse_local_set_refs(
    specs: &[String],
    policy: Option<&str>,
) -> Result<Vec<SetRefSpec>, CliError> {
    if specs.is_empty() {
        return Ok(Vec::new());
    }
    let Some(pol) = policy else {
        return Err(cli_err(
            EX_PARSE,
            "pkg/import",
            "--set-ref requires --policy <policy-hash>".to_string(),
        ));
    };
    if !is_hex64(pol) {
        return Err(cli_err(
            EX_PARSE,
            "pkg/import",
            "--policy must be 64-hex".to_string(),
        ));
    }

    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for s in specs {
        let (name, rhs) = s.split_once('=').ok_or_else(|| {
            cli_err(
                EX_PARSE,
                "pkg/import",
                "set-ref must be <refname>=<commit-hash|nil>[@<expected-old-hash|nil>]".to_string(),
            )
        })?;
        let name = name.trim();
        let rhs = rhs.trim();
        if name.is_empty() || rhs.is_empty() {
            return Err(cli_err(
                EX_PARSE,
                "pkg/import",
                "set-ref fields must be non-empty".to_string(),
            ));
        }
        if !seen.insert(name.to_string()) {
            return Err(cli_err(
                EX_PARSE,
                "pkg/import",
                format!("duplicate set-ref target: {name}"),
            ));
        }
        let (hash, expected_old) = match rhs.split_once('@') {
            None => (rhs, None),
            Some((h, eo)) => {
                let eo = eo.trim();
                if eo.is_empty() {
                    return Err(cli_err(
                        EX_PARSE,
                        "pkg/import",
                        "set-ref expected-old must be non-empty when @ is used".to_string(),
                    ));
                }
                (h.trim(), Some(eo))
            }
        };
        if hash != "nil" && !is_hex64(hash) {
            return Err(cli_err(
                EX_PARSE,
                "pkg/import",
                "set-ref hash must be 64-hex or `nil`".to_string(),
            ));
        }
        let expected_old = match expected_old {
            None => None,
            Some(eo) => {
                if eo != "nil" && !is_hex64(eo) {
                    return Err(cli_err(
                        EX_PARSE,
                        "pkg/import",
                        "set-ref expected-old must be 64-hex or `nil`".to_string(),
                    ));
                }
                Some(eo.to_string())
            }
        };
        out.push(SetRefSpec {
            name: name.to_string(),
            hash: hash.to_string(),
            policy: pol.to_string(),
            expected_old,
        });
    }
    Ok(out)
}

pub(crate) fn extract_vcs_patch_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":patch"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn extract_vcs_snapshot_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":snapshot"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn extract_vcs_commit_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":commit"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn extract_pkg_snapshot_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":snapshot"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn extract_pkg_export_bundle_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":bundle-h"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn extract_pkg_import_root(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":root"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn extract_pkg_publish_commit(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":commit"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn extract_pkg_lock_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":lock-h"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn extract_pkg_ok_bool(v: &Value) -> Option<bool> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":ok"))) {
        Some(Term::Bool(b)) => Some(*b),
        _ => None,
    }
}
