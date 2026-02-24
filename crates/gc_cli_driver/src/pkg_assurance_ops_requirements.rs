use super::*;

pub(super) fn parse_requirements_graph(
    req_term: &Term,
    known_obligations: &[String],
) -> Result<Vec<Term>, String> {
    let Term::Map(root) = req_term else {
        return Err("requirements graph must be a map".to_string());
    };
    let ty = root
        .get(&TermOrdKey(Term::symbol(":type")))
        .ok_or_else(|| "requirements graph missing :type".to_string())?;
    match ty {
        Term::Symbol(s) | Term::Str(s) if s == ":req/graph" || s == "req/graph" => {}
        _ => {
            return Err(format!(
                "requirements graph :type must be :req/graph, got {}",
                print_term(ty)
            ));
        }
    }
    let reqs = root
        .get(&TermOrdKey(Term::symbol(":requirements")))
        .ok_or_else(|| "requirements graph missing :requirements".to_string())?;
    let Term::Vector(items) = reqs else {
        return Err("requirements graph :requirements must be vector".to_string());
    };
    if items.is_empty() {
        return Err("requirements graph :requirements cannot be empty".to_string());
    }
    let known_ob: std::collections::BTreeSet<&str> =
        known_obligations.iter().map(String::as_str).collect();
    let mut out: Vec<Term> = Vec::new();
    for (i, req) in items.iter().enumerate() {
        let Term::Map(m) = req else {
            return Err(format!("requirements[{i}] must be map"));
        };
        let id = req_required_string(m, ":id", &format!("requirements[{i}]"))?;
        let level = normalize_symbol_like(&req_required_symbol_or_string(
            m,
            ":level",
            &format!("requirements[{i}]"),
        )?);
        if level != ":system" && level != ":hlr" && level != ":llr" {
            return Err(format!(
                "requirements[{i}] :level must be :system|:hlr|:llr"
            ));
        }
        let hazards = req_opt_str_vec(m, ":hazards", &format!("requirements[{i}]"))?;
        let parents = req_opt_str_vec(m, ":parents", &format!("requirements[{i}]"))?;
        let links_t = m
            .get(&TermOrdKey(Term::symbol(":links")))
            .ok_or_else(|| format!("requirements[{i}] missing :links"))?;
        let Term::Map(links) = links_t else {
            return Err(format!("requirements[{i}] :links must be map"));
        };
        let modules = parse_link_modules(links, i)?;
        let obligations = parse_link_symbols(links, ":obligations", i)?;
        for ob in &obligations {
            if !known_ob.contains(ob.as_str()) {
                return Err(format!(
                    "requirements[{i}] links obligation `{ob}` not present in package obligations"
                ));
            }
        }
        let evidence_kinds = parse_link_symbols(links, ":evidence-kinds", i)?
            .into_iter()
            .map(|k| normalize_symbol_like(&k))
            .collect::<Vec<_>>();
        if modules.is_empty() && obligations.is_empty() && evidence_kinds.is_empty() {
            return Err(format!(
                "requirements[{i}] :links must include modules, obligations, or evidence-kinds"
            ));
        }

        let mut links_out: BTreeMap<TermOrdKey, Term> = BTreeMap::new();
        if !modules.is_empty() {
            links_out.insert(TermOrdKey(Term::symbol(":modules")), Term::Vector(modules));
        }
        if !obligations.is_empty() {
            links_out.insert(
                TermOrdKey(Term::symbol(":obligations")),
                Term::Vector(obligations.into_iter().map(Term::symbol).collect()),
            );
        }
        if !evidence_kinds.is_empty() {
            links_out.insert(
                TermOrdKey(Term::symbol(":evidence-kinds")),
                Term::Vector(evidence_kinds.into_iter().map(Term::symbol).collect()),
            );
        }
        let req_out = Term::Map(
            [
                (TermOrdKey(Term::symbol(":id")), Term::Str(id)),
                (TermOrdKey(Term::symbol(":level")), Term::symbol(&level)),
                (
                    TermOrdKey(Term::symbol(":parents")),
                    Term::Vector(parents.into_iter().map(Term::Str).collect()),
                ),
                (
                    TermOrdKey(Term::symbol(":hazards")),
                    Term::Vector(hazards.into_iter().map(Term::Str).collect()),
                ),
                (TermOrdKey(Term::symbol(":links")), Term::Map(links_out)),
            ]
            .into_iter()
            .collect(),
        );
        out.push(req_out);
    }
    out.sort_by_key(print_term);
    Ok(out)
}

fn parse_link_modules(links: &BTreeMap<TermOrdKey, Term>, idx: usize) -> Result<Vec<Term>, String> {
    let Some(t) = links.get(&TermOrdKey(Term::symbol(":modules"))) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(format!(
            "requirements[{idx}] :links/:modules must be vector"
        ));
    };
    let mut out = Vec::new();
    for (j, x) in xs.iter().enumerate() {
        let Term::Map(mm) = x else {
            return Err(format!(
                "requirements[{idx}] :links/:modules[{j}] must be map"
            ));
        };
        let path = req_required_string(mm, ":path", &format!("requirements[{idx}]:modules[{j}]"))?;
        let exports = parse_link_symbols(mm, ":exports", idx)?;
        if exports.is_empty() {
            return Err(format!(
                "requirements[{idx}] :links/:modules[{j}] :exports cannot be empty"
            ));
        }
        out.push(Term::Map(
            [
                (TermOrdKey(Term::symbol(":path")), Term::Str(path)),
                (
                    TermOrdKey(Term::symbol(":exports")),
                    Term::Vector(exports.into_iter().map(Term::symbol).collect()),
                ),
            ]
            .into_iter()
            .collect(),
        ));
    }
    Ok(out)
}

fn parse_link_symbols(
    links: &BTreeMap<TermOrdKey, Term>,
    key: &str,
    idx: usize,
) -> Result<Vec<String>, String> {
    let Some(t) = links.get(&TermOrdKey(Term::symbol(key))) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(format!("requirements[{idx}] :links/{key} must be vector"));
    };
    let mut out = Vec::new();
    for (j, x) in xs.iter().enumerate() {
        match x {
            Term::Symbol(s) | Term::Str(s) => out.push(s.clone()),
            _ => {
                return Err(format!(
                    "requirements[{idx}] :links/{key}[{j}] must be symbol|string"
                ));
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn req_required_string(
    m: &BTreeMap<TermOrdKey, Term>,
    key: &str,
    what: &str,
) -> Result<String, String> {
    let t = m
        .get(&TermOrdKey(Term::symbol(key)))
        .ok_or_else(|| format!("{what} missing {key}"))?;
    match t {
        Term::Str(s) => {
            if s.trim().is_empty() {
                Err(format!("{what} {key} cannot be empty"))
            } else {
                Ok(s.clone())
            }
        }
        _ => Err(format!("{what} {key} must be string")),
    }
}

fn req_required_symbol_or_string(
    m: &BTreeMap<TermOrdKey, Term>,
    key: &str,
    what: &str,
) -> Result<String, String> {
    let t = m
        .get(&TermOrdKey(Term::symbol(key)))
        .ok_or_else(|| format!("{what} missing {key}"))?;
    match t {
        Term::Symbol(s) | Term::Str(s) => {
            if s.trim().is_empty() {
                Err(format!("{what} {key} cannot be empty"))
            } else {
                Ok(s.clone())
            }
        }
        _ => Err(format!("{what} {key} must be symbol|string")),
    }
}

fn req_opt_str_vec(
    m: &BTreeMap<TermOrdKey, Term>,
    key: &str,
    what: &str,
) -> Result<Vec<String>, String> {
    let Some(t) = m.get(&TermOrdKey(Term::symbol(key))) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(format!("{what} {key} must be vector"));
    };
    let mut out = Vec::new();
    for (i, x) in xs.iter().enumerate() {
        match x {
            Term::Str(s) => {
                if s.trim().is_empty() {
                    return Err(format!("{what} {key}[{i}] cannot be empty"));
                }
                out.push(s.clone());
            }
            _ => return Err(format!("{what} {key}[{i}] must be string")),
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

pub(super) fn normalize_requirement_ids(ids: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = if ids.is_empty() {
        vec!["TQ-BASELINE".to_string()]
    } else {
        ids.iter().map(|s| s.trim().to_string()).collect()
    };
    if out.iter().any(|s| s.is_empty()) {
        return Err("empty requirement id in --requirement".to_string());
    }
    out.sort();
    out.dedup();
    Ok(out)
}

pub(super) fn parse_tools(xs: &[String]) -> Result<Vec<(String, PathBuf)>, String> {
    if xs.is_empty() {
        let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
        return Ok(vec![("genesis-cli-driver".to_string(), exe)]);
    }
    let mut out = Vec::new();
    for raw in xs {
        let (name, path) = raw
            .split_once('=')
            .ok_or_else(|| format!("invalid --tool `{raw}`; expected name=path"))?;
        let name = name.trim();
        let path = path.trim();
        if name.is_empty() || path.is_empty() {
            return Err(format!(
                "invalid --tool `{raw}`; both name and path must be non-empty"
            ));
        }
        let pb = PathBuf::from(path);
        if !pb.is_file() {
            return Err(format!(
                "tool path does not exist or is not a file: {}",
                pb.display()
            ));
        }
        out.push((name.to_string(), pb));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    out.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
    Ok(out)
}

fn normalize_symbol_like(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with(':') {
        trimmed.to_string()
    } else {
        format!(":{trimmed}")
    }
}
