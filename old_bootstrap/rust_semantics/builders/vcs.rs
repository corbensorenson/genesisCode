use std::path::Path;

use gc_coreform::Term;

use crate::{CliError, EX_PARSE, cli_err};

pub(crate) fn mk_vcs_log_program(root: &str, max: u64) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/vcs-low::log")]);
    let payload = Term::Map(
        [
            (
                gc_coreform::TermOrdKey(Term::symbol(":root")),
                Term::Str(root.to_string()),
            ),
            (
                gc_coreform::TermOrdKey(Term::symbol(":max")),
                Term::Int((max as i64).into()),
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

pub(crate) fn mk_vcs_blame_program(
    snapshot: &str,
    sym: &str,
    path: Option<&str>,
) -> Result<Vec<Term>, CliError> {
    gc_vcs::validate_hex_hash(snapshot)
        .map_err(|e| cli_err(EX_PARSE, "vcs/blame", format!("invalid --snapshot: {e}")))?;
    if sym.trim().is_empty() {
        return Err(cli_err(EX_PARSE, "vcs/blame", "invalid --sym: empty value"));
    }

    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/vcs-low::blame")]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":snapshot")),
        Term::Str(snapshot.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":sym")),
        Term::Str(sym.to_string()),
    );
    if let Some(path) = path {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":path")),
            Term::Str(path.to_string()),
        );
    }
    let payload = Term::Map(m);
    let k = Term::list(vec![
        Term::symbol("fn"),
        Term::list(vec![Term::symbol("r")]),
        Term::list(vec![Term::symbol("core/effect::pure"), Term::symbol("r")]),
    ]);
    let perform = Term::list(vec![Term::symbol("core/effect::perform"), op, payload, k]);
    Ok(vec![
        Term::list(vec![Term::symbol("def"), Term::symbol("prog"), perform]),
        Term::symbol("prog"),
    ])
}

pub(crate) fn mk_vcs_why_program(
    snapshot: &str,
    sym: &str,
    op_sym: Option<&str>,
) -> Result<Vec<Term>, CliError> {
    gc_vcs::validate_hex_hash(snapshot)
        .map_err(|e| cli_err(EX_PARSE, "vcs/why", format!("invalid --snapshot: {e}")))?;
    if sym.trim().is_empty() {
        return Err(cli_err(EX_PARSE, "vcs/why", "invalid --sym: empty value"));
    }

    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/vcs-low::why")]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":snapshot")),
        Term::Str(snapshot.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":sym")),
        Term::Str(sym.to_string()),
    );
    if let Some(op_sym) = op_sym {
        if op_sym.trim().is_empty() {
            return Err(cli_err(EX_PARSE, "vcs/why", "invalid --op: empty value"));
        }
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":op")),
            Term::Str(op_sym.to_string()),
        );
    }
    let payload = Term::Map(m);
    let k = Term::list(vec![
        Term::symbol("fn"),
        Term::list(vec![Term::symbol("r")]),
        Term::list(vec![Term::symbol("core/effect::pure"), Term::symbol("r")]),
    ]);
    let perform = Term::list(vec![Term::symbol("core/effect::perform"), op, payload, k]);
    Ok(vec![
        Term::list(vec![Term::symbol("def"), Term::symbol("prog"), perform]),
        Term::symbol("prog"),
    ])
}

pub(crate) fn mk_vcs_diff_program(base: &str, to: &str, out: Option<&Path>, store: bool) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/vcs-low::diff")]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":base")),
        Term::Str(base.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":to")),
        Term::Str(to.to_string()),
    );
    if let Some(out) = out {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":out")),
            Term::Str(out.display().to_string()),
        );
    }
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":store")),
        Term::Bool(store),
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

pub(crate) fn mk_vcs_apply_program(base: &str, patch: &str, out: Option<&Path>, store: bool) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/vcs-low::apply")]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":base")),
        Term::Str(base.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":patch")),
        Term::Str(patch.to_string()),
    );
    if let Some(out) = out {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":out")),
            Term::Str(out.display().to_string()),
        );
    }
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":store")),
        Term::Bool(store),
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

pub(crate) fn mk_vcs_merge3_program(base: &str, left: &str, right: &str, out: Option<&Path>) -> Vec<Term> {
    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/vcs-low::merge3"),
    ]);
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":base")),
        Term::Str(base.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":left")),
        Term::Str(left.to_string()),
    );
    m.insert(
        gc_coreform::TermOrdKey(Term::symbol(":right")),
        Term::Str(right.to_string()),
    );
    if let Some(out) = out {
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":out")),
            Term::Str(out.display().to_string()),
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

pub(crate) fn mk_vcs_resolve_conflict_program(
    conflict: &str,
    strategy: Option<&str>,
    picks: &[String],
    sets: &[String],
    out: Option<&Path>,
) -> Result<Vec<Term>, CliError> {
    if strategy.is_none() && picks.is_empty() && sets.is_empty() {
        return Err(cli_err(
            EX_PARSE,
            "vcs/resolve-conflict",
            "must provide --strategy and/or --pick/--set overrides",
        ));
    }

    let op = Term::list(vec![
        Term::symbol("quote"),
        Term::symbol("core/vcs-low::resolve-conflict-legacy"),
    ]);

    let mut payload: std::collections::BTreeMap<gc_coreform::TermOrdKey, Term> =
        std::collections::BTreeMap::new();
    payload.insert(
        gc_coreform::TermOrdKey(Term::symbol(":conflict")),
        Term::Str(conflict.to_string()),
    );
    if let Some(s) = strategy {
        let s = s.trim();
        let sym = match s {
            "left" | ":left" => ":left",
            "right" | ":right" => ":right",
            "base" | ":base" => ":base",
            other => {
                return Err(cli_err(
                    EX_PARSE,
                    "vcs/resolve-conflict",
                    format!("unsupported --strategy {other} (expected left|right|base)"),
                ));
            }
        };
        payload.insert(
            gc_coreform::TermOrdKey(Term::symbol(":strategy")),
            Term::Str(sym.to_string()),
        );
    }
    if let Some(out) = out {
        payload.insert(
            gc_coreform::TermOrdKey(Term::symbol(":out")),
            Term::Str(out.display().to_string()),
        );
    }

    let mut res: std::collections::BTreeMap<String, Term> = std::collections::BTreeMap::new();
    for p in picks {
        let (opk, side) = p.split_once('=').ok_or_else(|| {
            cli_err(
                EX_PARSE,
                "vcs/resolve-conflict",
                format!("bad --pick {p}; expected op=left|right|base"),
            )
        })?;
        let opk = opk.trim();
        if opk.is_empty() {
            return Err(cli_err(
                EX_PARSE,
                "vcs/resolve-conflict",
                "bad --pick: empty op",
            ));
        }
        if res.contains_key(opk) {
            return Err(cli_err(
                EX_PARSE,
                "vcs/resolve-conflict",
                format!("duplicate resolution for op {opk}"),
            ));
        }
        let side = side.trim();
        let sym = match side {
            "left" | ":left" => ":left",
            "right" | ":right" => ":right",
            "base" | ":base" => ":base",
            other => {
                return Err(cli_err(
                    EX_PARSE,
                    "vcs/resolve-conflict",
                    format!("bad --pick {p}; unsupported side {other}"),
                ));
            }
        };
        res.insert(opk.to_string(), Term::Str(sym.to_string()));
    }
    for s in sets {
        let (opk, hv) = s.split_once('=').ok_or_else(|| {
            cli_err(
                EX_PARSE,
                "vcs/resolve-conflict",
                format!("bad --set {s}; expected op=<64-hex>"),
            )
        })?;
        let opk = opk.trim();
        if opk.is_empty() {
            return Err(cli_err(
                EX_PARSE,
                "vcs/resolve-conflict",
                "bad --set: empty op",
            ));
        }
        if res.contains_key(opk) {
            return Err(cli_err(
                EX_PARSE,
                "vcs/resolve-conflict",
                format!("duplicate resolution for op {opk}"),
            ));
        }
        let hv = hv.trim();
        gc_vcs::validate_hex_hash(hv)
            .map_err(|e| cli_err(EX_PARSE, "vcs/resolve-conflict", e.to_string()))?;
        res.insert(opk.to_string(), Term::Str(hv.to_string()));
    }
    if !res.is_empty() {
        let mut rm: std::collections::BTreeMap<gc_coreform::TermOrdKey, Term> =
            std::collections::BTreeMap::new();
        for (k, v) in res {
            rm.insert(gc_coreform::TermOrdKey(Term::Symbol(k)), v);
        }
        payload.insert(
            gc_coreform::TermOrdKey(Term::symbol(":resolutions")),
            Term::Map(rm),
        );
    }

    let payload = Term::Map(payload);
    let k = Term::list(vec![
        Term::symbol("fn"),
        Term::list(vec![Term::symbol("r")]),
        Term::list(vec![Term::symbol("core/effect::pure"), Term::symbol("r")]),
    ]);
    let perform = Term::list(vec![Term::symbol("core/effect::perform"), op, payload, k]);
    Ok(vec![
        Term::list(vec![Term::symbol("def"), Term::symbol("prog"), perform]),
        Term::symbol("prog"),
    ])
}
