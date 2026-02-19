use gc_coreform::{Term, TermOrdKey};
use gc_kernel::Value;

pub(crate) fn mk_store_put_program(artifact: &Term) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/store::put")]);
    let payload = Term::Map(
        [(
            TermOrdKey(Term::symbol(":artifact")),
            Term::list(vec![Term::symbol("quote"), artifact.clone()]),
        )]
        .into_iter()
        .collect(),
    );
    let k = Term::list(vec![
        Term::symbol("fn"),
        Term::list(vec![Term::symbol("r")]),
        Term::list(vec![Term::symbol("core/effect::pure"), Term::symbol("r")]),
    ]);
    let perform = Term::list(vec![Term::symbol("core/effect::perform"), op, payload, k]);
    vec![
        Term::list(vec![Term::symbol("def"), Term::symbol("prog"), perform]),
        Term::symbol("prog"),
    ]
}

pub(crate) fn mk_store_get_program(hash: &str) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/store::get")]);
    let payload = Term::Map(
        [(
            TermOrdKey(Term::symbol(":hash")),
            Term::Str(hash.to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let k = Term::list(vec![
        Term::symbol("fn"),
        Term::list(vec![Term::symbol("r")]),
        Term::list(vec![Term::symbol("core/effect::pure"), Term::symbol("r")]),
    ]);
    let perform = Term::list(vec![Term::symbol("core/effect::perform"), op, payload, k]);
    vec![
        Term::list(vec![Term::symbol("def"), Term::symbol("prog"), perform]),
        Term::symbol("prog"),
    ]
}

pub(crate) fn mk_store_has_program(hash: &str) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/store::has")]);
    let payload = Term::Map(
        [(
            TermOrdKey(Term::symbol(":hash")),
            Term::Str(hash.to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let k = Term::list(vec![
        Term::symbol("fn"),
        Term::list(vec![Term::symbol("r")]),
        Term::list(vec![Term::symbol("core/effect::pure"), Term::symbol("r")]),
    ]);
    let perform = Term::list(vec![Term::symbol("core/effect::perform"), op, payload, k]);
    vec![
        Term::list(vec![Term::symbol("def"), Term::symbol("prog"), perform]),
        Term::symbol("prog"),
    ]
}

pub(crate) fn mk_store_verify_program(hash: Option<&str>) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/store::verify"),
    ]);
    let mut payload = std::collections::BTreeMap::new();
    payload.insert(
        TermOrdKey(Term::symbol(":hash")),
        hash.map(|h| Term::Str(h.to_string())).unwrap_or(Term::Nil),
    );
    let payload = Term::Map(payload);
    let k = Term::list(vec![
        Term::symbol("fn"),
        Term::list(vec![Term::symbol("r")]),
        Term::list(vec![Term::symbol("core/effect::pure"), Term::symbol("r")]),
    ]);
    let perform = Term::list(vec![Term::symbol("core/effect::perform"), op, payload, k]);
    vec![
        Term::list(vec![Term::symbol("def"), Term::symbol("prog"), perform]),
        Term::symbol("prog"),
    ]
}

pub(crate) fn extract_store_put_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&TermOrdKey(Term::symbol(":hash"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn extract_store_has_present(v: &Value) -> Option<bool> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&TermOrdKey(Term::symbol(":present"))) {
        Some(Term::Bool(b)) => Some(*b),
        _ => None,
    }
}

pub(crate) fn extract_store_get_artifact(v: &Value) -> Option<Term> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    m.get(&TermOrdKey(Term::symbol(":artifact"))).cloned()
}

pub(crate) fn extract_store_verify_checked(v: &Value) -> Option<u64> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&TermOrdKey(Term::symbol(":checked"))) {
        Some(Term::Int(n)) => n.to_string().parse::<u64>().ok(),
        _ => None,
    }
}
