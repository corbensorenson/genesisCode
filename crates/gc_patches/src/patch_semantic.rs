use super::*;

pub(super) fn usize_to_int_term(x: usize) -> Result<Term, PatchError> {
    let i = i64::try_from(x).map_err(|_| PatchError::Validate("index out of range".to_string()))?;
    Ok(Term::Int(i.into()))
}

pub(super) fn path_steps_to_term(path: &[PathStep]) -> Result<Term, PatchError> {
    let mut steps = Vec::with_capacity(path.len());
    for st in path {
        let t = match st {
            PathStep::Form(i) => Term::Vector(vec![Term::symbol(":form"), usize_to_int_term(*i)?]),
            PathStep::PairCar => Term::Vector(vec![Term::symbol(":pair-car")]),
            PathStep::PairCdr => Term::Vector(vec![Term::symbol(":pair-cdr")]),
            PathStep::Vec(i) => Term::Vector(vec![Term::symbol(":vec"), usize_to_int_term(*i)?]),
            PathStep::Map(k) => Term::Vector(vec![Term::symbol(":map"), k.clone()]),
        };
        steps.push(t);
    }
    Ok(Term::Vector(steps))
}

pub(super) fn hash32_hex(h: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for b in h {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

pub(super) fn semantic_node_id(module_path: &str, path: &[PathStep]) -> Result<String, PatchError> {
    let path_term = path_steps_to_term(path)?;
    let path_repr = print_term(&path_term);
    let mut h = blake3::Hasher::new();
    h.update(b"GCv0.2\0semantic-node-id\0");
    h.update(module_path.as_bytes());
    h.update(b"\0");
    h.update(path_repr.as_bytes());
    Ok(h.finalize().to_hex().to_string())
}

pub(super) fn term_tag(t: &Term) -> &'static str {
    match t {
        Term::Nil => "nil",
        Term::Bool(_) => "bool",
        Term::Int(_) => "int",
        Term::Str(_) => "str",
        Term::Bytes(_) => "bytes",
        Term::Symbol(_) => "sym",
        Term::Pair(_, _) => "pair",
        Term::Vector(_) => "vec",
        Term::Map(_) => "map",
    }
}

pub(super) fn collect_term_nodes(
    module_path: &str,
    path: &mut Vec<PathStep>,
    t: &Term,
    out: &mut Vec<SemanticNodeRecord>,
) -> Result<(), PatchError> {
    let node_id = semantic_node_id(module_path, path)?;
    let path_term = path_steps_to_term(path)?;
    let path_repr = print_term(&path_term);
    out.push(SemanticNodeRecord {
        module_path: module_path.to_string(),
        node_id,
        path: path_term,
        path_repr,
        term_tag: term_tag(t).to_string(),
        term_hash: hash32_hex(hash_term(t)),
    });
    match t {
        Term::Pair(a, d) => {
            path.push(PathStep::PairCar);
            collect_term_nodes(module_path, path, a, out)?;
            path.pop();
            path.push(PathStep::PairCdr);
            collect_term_nodes(module_path, path, d, out)?;
            path.pop();
        }
        Term::Vector(xs) => {
            for (i, child) in xs.iter().enumerate() {
                path.push(PathStep::Vec(i));
                collect_term_nodes(module_path, path, child, out)?;
                path.pop();
            }
        }
        Term::Map(m) => {
            for (k, child) in m {
                path.push(PathStep::Map(k.0.clone()));
                collect_term_nodes(module_path, path, child, out)?;
                path.pop();
            }
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn semantic_node_index_for_forms(
    module_path: &str,
    forms: &[Term],
) -> Result<Vec<SemanticNodeRecord>, PatchError> {
    let mut out = Vec::new();
    for (i, form) in forms.iter().enumerate() {
        let mut path = vec![PathStep::Form(i)];
        collect_term_nodes(module_path, &mut path, form, &mut out)?;
    }
    Ok(out)
}

pub(super) fn resolve_node_id_path(
    module_path: &str,
    forms: &[Term],
    node_id: &str,
) -> Result<Vec<PathStep>, PatchError> {
    let nodes = semantic_node_index_for_forms(module_path, forms)?;
    let mut matches = nodes
        .into_iter()
        .filter(|n| n.node_id == node_id)
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return Err(PatchError::Validate(format!(
            "replace-node-id unknown :node-id {node_id} in module {module_path}"
        )));
    }
    if matches.len() > 1 {
        return Err(PatchError::Validate(format!(
            "replace-node-id ambiguous :node-id {node_id} in module {module_path}"
        )));
    }
    parse_path(&matches.remove(0).path)
}

pub(super) fn semantic_node_index_for_module_with_frontend(
    module_path: &str,
    src: &str,
    frontend: &CoreformFrontend,
    step_limit: StepLimit,
    mem_limits: MemLimits,
) -> Result<Vec<SemanticNodeRecord>, PatchError> {
    let forms = parse_canonicalize_module_src(src, frontend, step_limit, mem_limits)?;
    semantic_node_index_for_forms(module_path, &forms)
}
