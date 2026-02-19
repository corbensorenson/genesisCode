use std::path::Path;

use gc_coreform::Term;

use crate::SetRefSpec;

pub(crate) fn mk_pkg_init_program(
    workspace: &str,
    lock: &Path,
    policy: &str,
    registry_default: Option<&str>,
) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/pkg-low::init"),
    ]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":workspace")),
        Term::Str(workspace.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":lock")),
        Term::Str(lock.display().to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":policy")),
        Term::Str(policy.to_string()),
    );
    if let Some(rd) = registry_default {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":registry-default")),
            Term::Str(rd.to_string()),
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

pub(crate) fn mk_pkg_add_program(
    lock: &Path,
    name: &str,
    selector: &str,
    update_policy: &str,
    registry: Option<&str>,
    strategy: Option<&str>,
    tag_policy: Option<&str>,
) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/pkg-low::add"),
    ]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":lock")),
        Term::Str(lock.display().to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":name")),
        Term::Str(name.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":selector")),
        Term::Str(selector.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":update-policy")),
        Term::Str(update_policy.to_string()),
    );
    if let Some(r) = registry {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":registry")),
            Term::Str(r.to_string()),
        );
    }
    if let Some(s) = strategy {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":strategy")),
            Term::Str(s.to_string()),
        );
    }
    if let Some(tp) = tag_policy {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":tag-policy")),
            Term::Str(tp.to_string()),
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

pub(crate) fn mk_pkg_lock_program(lock: &Path, strict: bool) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/pkg-low::lock"),
    ]);
    let payload = Term::Map(
        [
            (
                gc_coreform::TermOrdKey(Term::symbol(":lock")),
                Term::Str(lock.display().to_string()),
            ),
            (
                gc_coreform::TermOrdKey(Term::symbol(":strict")),
                Term::Bool(strict),
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

pub(crate) fn mk_pkg_update_program(lock: &Path) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/pkg-low::update"),
    ]);
    let payload = Term::Map(
        [(
            gc_coreform::TermOrdKey(Term::symbol(":lock")),
            Term::Str(lock.display().to_string()),
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

pub(crate) fn mk_pkg_install_program(lock: &Path, frozen: bool, strict: bool) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/pkg-low::install"),
    ]);
    let payload = Term::Map(
        [
            (
                gc_coreform::TermOrdKey(Term::symbol(":lock")),
                Term::Str(lock.display().to_string()),
            ),
            (
                gc_coreform::TermOrdKey(Term::symbol(":frozen")),
                Term::Bool(frozen),
            ),
            (
                gc_coreform::TermOrdKey(Term::symbol(":strict")),
                Term::Bool(strict),
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

pub(crate) fn mk_pkg_verify_program(lock: &Path) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/pkg-low::verify"),
    ]);
    let payload = Term::Map(
        [(
            gc_coreform::TermOrdKey(Term::symbol(":lock")),
            Term::Str(lock.display().to_string()),
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

pub(crate) fn mk_pkg_list_program(lock: &Path) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/pkg-low::list"),
    ]);
    let payload = Term::Map(
        [(
            gc_coreform::TermOrdKey(Term::symbol(":lock")),
            Term::Str(lock.display().to_string()),
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

pub(crate) fn mk_pkg_info_program(lock: &Path, name: &str) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/pkg-low::info"),
    ]);
    let payload = Term::Map(
        [
            (
                gc_coreform::TermOrdKey(Term::symbol(":lock")),
                Term::Str(lock.display().to_string()),
            ),
            (
                gc_coreform::TermOrdKey(Term::symbol(":name")),
                Term::Str(name.to_string()),
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

pub(crate) fn mk_pkg_snapshot_program(pkg: &Path) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/pkg-low::snapshot"),
    ]);
    let payload = Term::Map(
        [(
            gc_coreform::TermOrdKey(Term::symbol(":pkg")),
            Term::Str(pkg.display().to_string()),
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

pub(crate) fn mk_pkg_publish_program(
    remote: &str,
    refname: &str,
    policy_h: &str,
    expected_old: Option<&str>,
    depth: u64,
    commit: Option<&str>,
) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/pkg-low::publish"),
    ]);
    let mut payload = std::collections::BTreeMap::new();
    payload.insert(
        gc_coreform::TermOrdKey(Term::symbol(":remote")),
        Term::Str(remote.to_string()),
    );
    payload.insert(
        gc_coreform::TermOrdKey(Term::symbol(":ref")),
        Term::Str(refname.to_string()),
    );
    payload.insert(
        gc_coreform::TermOrdKey(Term::symbol(":policy")),
        Term::Str(policy_h.to_string()),
    );
    if let Some(e) = expected_old {
        let v = if e == "nil" {
            Term::Nil
        } else {
            Term::Str(e.to_string())
        };
        payload.insert(gc_coreform::TermOrdKey(Term::symbol(":expected-old")), v);
    }
    if depth > 0 {
        payload.insert(
            gc_coreform::TermOrdKey(Term::symbol(":depth")),
            Term::Int((depth as i64).into()),
        );
    }
    if let Some(h) = commit {
        payload.insert(
            gc_coreform::TermOrdKey(Term::symbol(":commit")),
            Term::Str(h.to_string()),
        );
    }
    let k = Term::list(vec![
        Term::symbol("fn"),
        Term::list(vec![Term::symbol("r")]),
        Term::list(vec![Term::symbol("core/effect::pure"), Term::symbol("r")]),
    ]);
    let perform = Term::list(vec![
        Term::symbol("core/effect::perform"),
        op,
        Term::Map(payload),
        k,
    ]);
    vec![
        Term::list(vec![Term::symbol("def"), Term::symbol("prog"), perform]),
        Term::symbol("prog"),
    ]
}

pub(crate) fn mk_gpk_export_program(
    root: &str,
    out: &Path,
    full: bool,
    depth: u64,
    include_evidence: &str,
    include_deps: &str,
    include_refs: &[String],
) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/gpk-low::export"),
    ]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":root")),
        Term::Str(root.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":out")),
        Term::Str(out.display().to_string()),
    );
    if full {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":mode")),
            Term::Str(":full".to_string()),
        );
    } else {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":mode")),
            Term::Str(":shallow".to_string()),
        );
    }
    if depth > 0 {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":depth")),
            Term::Int((depth as i64).into()),
        );
    }
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":include-evidence")),
        Term::Str(include_evidence.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":include-deps")),
        Term::Str(include_deps.to_string()),
    );
    if !include_refs.is_empty() {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":refs")),
            Term::Vector(include_refs.iter().cloned().map(Term::Str).collect()),
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

pub(crate) fn mk_gpk_import_program(input: &Path, set_refs: &[SetRefSpec]) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/gpk-low::import"),
    ]);
    let mut payload_m = std::collections::BTreeMap::new();
    payload_m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":in")),
        Term::Str(input.display().to_string()),
    );
    if !set_refs.is_empty() {
        let mut entries = Vec::with_capacity(set_refs.len());
        for sr in set_refs {
            let mut em = std::collections::BTreeMap::new();
            em.insert(
                gc_coreform::TermOrdKey(Term::symbol(":name")),
                Term::Str(sr.name.clone()),
            );
            em.insert(
                gc_coreform::TermOrdKey(Term::symbol(":hash")),
                if sr.hash == "nil" {
                    Term::Nil
                } else {
                    Term::Str(sr.hash.clone())
                },
            );
            em.insert(
                gc_coreform::TermOrdKey(Term::symbol(":policy")),
                Term::Str(sr.policy.clone()),
            );
            if let Some(exp) = &sr.expected_old {
                em.insert(
                    gc_coreform::TermOrdKey(Term::symbol(":expected-old")),
                    if exp == "nil" {
                        Term::Nil
                    } else {
                        Term::Str(exp.clone())
                    },
                );
            }
            entries.push(Term::Map(em));
        }
        payload_m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":set-refs")),
            Term::Vector(entries),
        );
    }
    let payload = Term::Map(payload_m);
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
