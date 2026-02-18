use std::path::Path;

use gc_coreform::Term;
use crate::SetRefSpec;

pub(crate) fn mk_sync_pull_program(
    remote: &str,
    refs: &[String],
    roots: &[String],
    depth: u64,
    force: bool,
) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/sync::pull")]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":remote")),
        Term::Str(remote.to_string()),
    );
    if !refs.is_empty() {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":refs")),
            Term::Vector(refs.iter().cloned().map(Term::Str).collect()),
        );
    }
    if !roots.is_empty() {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":roots")),
            Term::Vector(roots.iter().cloned().map(Term::Str).collect()),
        );
    }
    if depth > 0 {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":depth")),
            Term::Int((depth as i64).into()),
        );
    }
    if force {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":force")),
            Term::Bool(true),
        );
    }
    let payload = Term::Map(m);
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

pub(crate) fn mk_sync_push_program(
    remote: &str,
    roots: &[String],
    depth: u64,
    set_refs: &[SetRefSpec],
) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/sync::push")]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":remote")),
        Term::Str(remote.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":roots")),
        Term::Vector(roots.iter().cloned().map(Term::Str).collect()),
    );
    if depth > 0 {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":depth")),
            Term::Int((depth as i64).into()),
        );
    }
    if !set_refs.is_empty() {
        let mut out = Vec::new();
        for sr in set_refs {
            let mut mm = std::collections::BTreeMap::new();
            mm.insert(
                gc_coreform::TermOrdKey(Term::symbol(":name")),
                Term::Str(sr.name.clone()),
            );
            mm.insert(
                gc_coreform::TermOrdKey(Term::symbol(":hash")),
                Term::Str(sr.hash.clone()),
            );
            mm.insert(
                gc_coreform::TermOrdKey(Term::symbol(":policy")),
                Term::Str(sr.policy.clone()),
            );
            if let Some(e) = &sr.expected_old {
                let v = if e == "nil" {
                    Term::Nil
                } else {
                    Term::Str(e.clone())
                };
                mm.insert(gc_coreform::TermOrdKey(Term::symbol(":expected-old")), v);
            }
            out.push(Term::Map(mm));
        }
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":set-refs")),
            Term::Vector(out),
        );
    }
    let payload = Term::Map(m);
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

pub(crate) fn mk_gc_plan_program(
    lock: &Path,
    pins: &Path,
    depth: u64,
    include_lock: bool,
    include_refs: bool,
) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/gc-low::plan")]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":lock")),
        Term::Str(lock.display().to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":pins")),
        Term::Str(pins.display().to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":depth")),
        Term::Int((depth as i64).into()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":include-lock")),
        Term::Bool(include_lock),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":include-refs")),
        Term::Bool(include_refs),
    );
    let payload = Term::Map(m);
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

pub(crate) fn mk_gc_run_program(
    lock: &Path,
    pins: &Path,
    depth: u64,
    include_lock: bool,
    include_refs: bool,
    quarantine: bool,
    quarantine_dir: Option<&Path>,
) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/gc-low::run")]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":lock")),
        Term::Str(lock.display().to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":pins")),
        Term::Str(pins.display().to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":depth")),
        Term::Int((depth as i64).into()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":include-lock")),
        Term::Bool(include_lock),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":include-refs")),
        Term::Bool(include_refs),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":quarantine")),
        Term::Bool(quarantine),
    );
    if let Some(qd) = quarantine_dir {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":quarantine-dir")),
            Term::Str(qd.display().to_string()),
        );
    }
    let payload = Term::Map(m);
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

pub(crate) fn mk_gc_pin_program(target: &str, pins: &Path) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/gc-low::pin")]);
    let payload = Term::Map(
        [
            (
                gc_coreform::TermOrdKey(Term::symbol(":target")),
                Term::Str(target.to_string()),
            ),
            (
                gc_coreform::TermOrdKey(Term::symbol(":pins")),
                Term::Str(pins.display().to_string()),
            ),
        ]
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

pub(crate) fn mk_gc_unpin_program(target: &str, pins: &Path) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/gc-low::unpin")]);
    let payload = Term::Map(
        [
            (
                gc_coreform::TermOrdKey(Term::symbol(":target")),
                Term::Str(target.to_string()),
            ),
            (
                gc_coreform::TermOrdKey(Term::symbol(":pins")),
                Term::Str(pins.display().to_string()),
            ),
        ]
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

pub(crate) fn mk_gc_purge_program(ttl_days: u64, quarantine_dir: Option<&Path>) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/gc-low::purge")]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":ttl-days")),
        Term::Int((ttl_days as i64).into()),
    );
    if let Some(qd) = quarantine_dir {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":quarantine-dir")),
            Term::Str(qd.display().to_string()),
        );
    }
    let payload = Term::Map(m);
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
