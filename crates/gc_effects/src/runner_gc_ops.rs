use super::*;

pub(super) fn gpk_export_closure_local(
    store: &ArtifactStore,
    root: &str,
    opts: GpkClosureOptions<'_>,
    out: &mut std::collections::BTreeSet<String>,
    error_tok: SealId,
    op: &str,
) -> Result<(), Value> {
    use std::collections::{HashSet, VecDeque};

    let mut helper_ctx = EvalCtx::new();
    let helper_prelude = build_prelude(&mut helper_ctx);
    let helper_ref_plan_fn = helper_prelude
        .env
        .get("core/vcs/reach::artifact-ref-plan")
        .ok_or_else(|| {
            mk_error(
                error_tok,
                "core/gpk/planner-missing",
                "missing prelude binding core/vcs/reach::artifact-ref-plan".to_string(),
                Some(op),
            )
        })?;

    let mut q: VecDeque<(String, u64, bool)> = VecDeque::new();
    q.push_back((root.to_string(), opts.depth, true));
    let mut seen: HashSet<String> = HashSet::new();
    let mut obj_count: u64 = 0;

    while let Some((h, dleft, is_root)) = q.pop_front() {
        if !seen.insert(h.clone()) {
            continue;
        }
        obj_count = obj_count.saturating_add(1);
        if obj_count > 50_000 {
            return Err(mk_error(
                error_tok,
                "core/sync/too-many-objects",
                "closure exceeded 50k objects".to_string(),
                Some(op),
            ));
        }
        if !store.path_for(&h).exists() {
            return Err(mk_error(
                error_tok,
                "core/store/not-found",
                format!("artifact not found: {h}"),
                Some(op),
            ));
        }
        if store.verify_hex(&h).is_err() {
            return Err(mk_error(
                error_tok,
                "core/store/corruption",
                format!("artifact store corruption: {h}"),
                Some(op),
            ));
        }
        out.insert(h.clone());

        let t = match store_get_term(store, &h) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let is_evidence_artifact = gc_vcs::Evidence::from_term(&t).is_ok();
        let include_commit_evidence = match opts.include_evidence {
            GpkIncludeEvidence::None => false,
            // Required mode includes evidence directly referenced by the root object, and once an
            // evidence artifact is traversed we continue following its internal evidence refs.
            GpkIncludeEvidence::Required => is_root || is_evidence_artifact,
            GpkIncludeEvidence::All => true,
        };
        let follow_deps = match opts.include_deps {
            GpkIncludeDeps::None => false,
            GpkIncludeDeps::Locked => opts
                .root_snapshot_for_locked_deps
                .map(|hh| hh.eq_ignore_ascii_case(&h))
                .unwrap_or(false),
            GpkIncludeDeps::All => true,
        };

        let mut opts_map = BTreeMap::new();
        opts_map.insert(
            TermOrdKey(Term::symbol(":include-evidence")),
            Term::Bool(include_commit_evidence),
        );
        opts_map.insert(
            TermOrdKey(Term::symbol(":include-deps")),
            Term::Bool(follow_deps),
        );
        opts_map.insert(
            TermOrdKey(Term::symbol(":include-parents")),
            Term::Bool(opts.mode == GpkMode::Full && dleft > 0),
        );
        let opts_term = Term::Map(opts_map);

        let plan_term = helper_ref_plan_fn
            .clone()
            .apply(&mut helper_ctx, Value::Data(t.clone()))
            .and_then(|f| f.apply(&mut helper_ctx, Value::Data(opts_term)))
            .map(|v| v.to_term_for_log(helper_ctx.protocol.map(|p| p.error)))
            .map_err(|e| {
                mk_error(
                    error_tok,
                    "core/gpk/planner-error",
                    format!("core/vcs/reach::artifact-ref-plan failed: {e}"),
                    Some(op),
                )
            })?;
        let (refs_to_follow, parent_refs) = gpk_ref_plan_from_term(&plan_term);
        for x in refs_to_follow {
            q.push_back((x, dleft, false));
        }

        if dleft > 0 {
            for p in parent_refs {
                q.push_back((p, dleft - 1, false));
            }
        }
    }
    Ok(())
}

fn gpk_ref_hashes_from_term(t: &Term) -> Vec<String> {
    let Term::Vector(xs) = t else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for x in xs {
        let s = match x {
            Term::Str(s) | Term::Symbol(s) => s,
            _ => continue,
        };
        if gc_vcs::validate_hex_hash(s).is_ok() {
            out.push(s.to_ascii_lowercase());
        }
    }
    out
}

fn gpk_ref_plan_from_term(t: &Term) -> (Vec<String>, Vec<String>) {
    let Term::Map(m) = t else {
        return (Vec::new(), Vec::new());
    };
    let refs = m
        .get(&TermOrdKey(Term::symbol(":refs")))
        .map(gpk_ref_hashes_from_term)
        .unwrap_or_default();
    let parents = m
        .get(&TermOrdKey(Term::symbol(":parents")))
        .map(gpk_ref_hashes_from_term)
        .unwrap_or_default();
    (refs, parents)
}

#[expect(
    clippy::too_many_arguments,
    reason = "gc planning receives explicit inputs to keep deterministic source accounting visible"
)]
pub(super) fn gc_build_sources(
    refs: Option<&RefsDb>,
    base_dir: &Path,
    lock_s: &str,
    pins_s: &str,
    include_lock: bool,
    include_refs: bool,
    error_tok: SealId,
    op: &str,
) -> Result<(Vec<Term>, Term, Term), Value> {
    let mut ref_entries: Vec<Term> = Vec::new();
    if include_refs && let Some(rdb) = refs {
        match rdb.list(None) {
            Ok(list) => {
                for r in list {
                    let mut m = BTreeMap::new();
                    m.insert(TermOrdKey(Term::symbol(":name")), Term::Str(r.name));
                    m.insert(
                        TermOrdKey(Term::symbol(":hash")),
                        r.hash.map(Term::Str).unwrap_or(Term::Nil),
                    );
                    ref_entries.push(Term::Map(m));
                }
            }
            Err(e) => {
                return Err(mk_error(
                    error_tok,
                    "core/gc/refs-io-error",
                    e.to_string(),
                    Some(op),
                ));
            }
        }
    }

    let mut lock_entries_term: Vec<Term> = Vec::new();
    let mut lock_artifacts_term: BTreeMap<TermOrdKey, Term> = BTreeMap::new();
    if include_lock
        && let Ok(lock_path) = sandbox_path_read(base_dir, lock_s)
        && lock_path.exists()
    {
        match gc_pkg::GenesisLock::load(&lock_path) {
            Ok(lk) => {
                for (_, le) in lk.locked {
                    let mut m = BTreeMap::new();
                    m.insert(
                        TermOrdKey(Term::symbol(":commit")),
                        le.commit.map(Term::Str).unwrap_or(Term::Nil),
                    );
                    m.insert(
                        TermOrdKey(Term::symbol(":snapshot")),
                        Term::Str(le.snapshot),
                    );
                    lock_entries_term.push(Term::Map(m));
                }
                for (k, v) in lk.artifacts {
                    lock_artifacts_term.insert(TermOrdKey(Term::Str(k)), Term::Str(v));
                }
            }
            Err(e) => {
                return Err(mk_error(
                    error_tok,
                    "core/gc/bad-lock",
                    format!("{e}"),
                    Some(op),
                ));
            }
        }
    }
    let lock_info = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":lock")),
                Term::Str(lock_s.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":locked")),
                Term::Vector(lock_entries_term),
            ),
            (
                TermOrdKey(Term::symbol(":artifacts")),
                Term::Map(lock_artifacts_term),
            ),
        ]
        .into_iter()
        .collect(),
    );

    let pins = match gc_pins_load(base_dir, pins_s) {
        Ok(p) => p,
        Err(e) => return Err(mk_error(error_tok, "core/gc/bad-pins", e, Some(op))),
    };
    let mut keep_refs_term: Vec<Term> = Vec::new();
    for rname in &pins.keep_refs {
        let Some(rdb) = refs else {
            return Err(mk_error(
                error_tok,
                "core/gc/missing-refs-db",
                "pins.keep_refs requires refs db".to_string(),
                Some(op),
            ));
        };
        let cur = match rdb.get(rname) {
            Ok(h) => h,
            Err(e) => {
                return Err(mk_error(
                    error_tok,
                    "core/gc/refs-io-error",
                    e.to_string(),
                    Some(op),
                ));
            }
        };
        let Some(h) = cur else {
            return Err(mk_error(
                error_tok,
                "core/gc/ref-not-found",
                format!("pinned ref not found: {rname}"),
                Some(op),
            ));
        };
        keep_refs_term.push(Term::Map(
            [
                (TermOrdKey(Term::symbol(":name")), Term::Str(rname.clone())),
                (TermOrdKey(Term::symbol(":hash")), Term::Str(h)),
            ]
            .into_iter()
            .collect(),
        ));
    }
    let pins_info = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":keep")),
                Term::Vector(pins.keep.into_iter().map(Term::Str).collect()),
            ),
            (
                TermOrdKey(Term::symbol(":keep-refs")),
                Term::Vector(keep_refs_term),
            ),
        ]
        .into_iter()
        .collect(),
    );

    Ok((ref_entries, lock_info, pins_info))
}

pub(super) fn gc_roots_plan_from_sources(
    refs_entries: &[Term],
    lock_info: &Term,
    pins_info: &Term,
    include_lock: bool,
    include_refs: bool,
    error_tok: SealId,
    op: &str,
) -> Result<(Vec<String>, Vec<Term>), Value> {
    let mut helper_ctx = EvalCtx::new();
    let helper_prelude = build_prelude(&mut helper_ctx);
    let roots_plan_fn = helper_prelude
        .env
        .get("core/gc/reach::roots-plan")
        .ok_or_else(|| {
            mk_error(
                error_tok,
                "core/gc/planner-missing",
                "missing prelude binding core/gc/reach::roots-plan".to_string(),
                Some(op),
            )
        })?;

    let plan_term = roots_plan_fn
        .apply(
            &mut helper_ctx,
            Value::Data(Term::Vector(refs_entries.to_vec())),
        )
        .and_then(|f| f.apply(&mut helper_ctx, Value::Data(lock_info.clone())))
        .and_then(|f| f.apply(&mut helper_ctx, Value::Data(pins_info.clone())))
        .and_then(|f| f.apply(&mut helper_ctx, Value::Data(Term::Bool(include_lock))))
        .and_then(|f| f.apply(&mut helper_ctx, Value::Data(Term::Bool(include_refs))))
        .map(|v| v.to_term_for_log(helper_ctx.protocol.map(|p| p.error)))
        .map_err(|e| {
            mk_error(
                error_tok,
                "core/gc/planner-error",
                format!("core/gc/reach::roots-plan failed: {e}"),
                Some(op),
            )
        })?;

    let Term::Map(m) = plan_term else {
        return Err(mk_error(
            error_tok,
            "core/gc/planner-error",
            "gc roots planner must return a map".to_string(),
            Some(op),
        ));
    };
    let roots = m
        .get(&TermOrdKey(Term::symbol(":roots")))
        .map(gpk_ref_hashes_from_term)
        .unwrap_or_default();
    let roots_meta = m
        .get(&TermOrdKey(Term::symbol(":roots-meta")))
        .and_then(|t| match t {
            Term::Vector(v) => Some(v.clone()),
            _ => None,
        })
        .unwrap_or_default();

    Ok((roots, roots_meta))
}

// -----------------------------------------------------------------------------
// GC helpers (pins + store lock + store scan)
// -----------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(super) struct GcPins {
    pub(super) keep: Vec<String>,
    pub(super) keep_refs: Vec<String>,
}

impl GcPins {
    pub(super) fn empty() -> Self {
        Self {
            keep: Vec::new(),
            keep_refs: Vec::new(),
        }
    }
}

pub(super) fn gc_normalize_hash(s: &str) -> Option<String> {
    let ss = s.strip_prefix("h:").unwrap_or(s).trim();
    if gc_vcs::validate_hex_hash(ss).is_err() {
        return None;
    }
    Some(ss.to_ascii_lowercase())
}

pub(super) fn gc_pins_load(base_dir: &Path, pins_path: &str) -> Result<GcPins, String> {
    let p = sandbox_path_allow_missing(base_dir, pins_path, false).map_err(|e| format!("{e}"))?;
    if !p.exists() {
        return Ok(GcPins::empty());
    }
    let bytes = std::fs::read(&p).map_err(|e| format!("pins read failed: {e}"))?;
    let s = String::from_utf8(bytes).map_err(|_| "pins file is not utf-8".to_string())?;
    let v: toml::Value = toml::from_str(&s).map_err(|e| format!("pins toml parse: {e}"))?;

    let version = v.get("version").and_then(|x| x.as_integer()).unwrap_or(1);
    if version != 1 {
        return Err(format!("unsupported pins version: {version}"));
    }

    let pins_tbl = v
        .get("pins")
        .and_then(|x| x.as_table())
        .ok_or_else(|| "pins.toml missing [pins] table".to_string())?;

    let mut keep: Vec<String> = Vec::new();
    if let Some(arr) = pins_tbl.get("keep").and_then(|x| x.as_array()) {
        for x in arr {
            let Some(s) = x.as_str() else {
                return Err("pins.keep entries must be strings".to_string());
            };
            let Some(h) = gc_normalize_hash(s) else {
                return Err(format!("pins.keep contains invalid hash: {s}"));
            };
            keep.push(h);
        }
    }

    let mut keep_refs: Vec<String> = Vec::new();
    if let Some(arr) = pins_tbl.get("keep_refs").and_then(|x| x.as_array()) {
        for x in arr {
            let Some(s) = x.as_str() else {
                return Err("pins.keep_refs entries must be strings".to_string());
            };
            if !s.starts_with("refs/") {
                return Err(format!("pins.keep_refs must start with refs/: {s}"));
            }
            keep_refs.push(s.to_string());
        }
    }

    keep.sort();
    keep.dedup();
    keep_refs.sort();
    keep_refs.dedup();

    Ok(GcPins { keep, keep_refs })
}

pub(super) fn gc_pins_write(path: &Path, pins: &GcPins) -> Result<(), EffectsError> {
    // Stable writer: fixed key order, single-line arrays.
    fn write_arr(buf: &mut String, xs: &[String]) {
        buf.push('[');
        for (i, x) in xs.iter().enumerate() {
            if i != 0 {
                buf.push_str(", ");
            }
            buf.push('"');
            for c in x.chars() {
                match c {
                    '\\' => buf.push_str("\\\\"),
                    '"' => buf.push_str("\\\""),
                    '\n' => buf.push_str("\\n"),
                    '\r' => buf.push_str("\\r"),
                    '\t' => buf.push_str("\\t"),
                    other => buf.push(other),
                }
            }
            buf.push('"');
        }
        buf.push(']');
    }

    let mut keep = pins.keep.clone();
    keep.sort();
    keep.dedup();
    let mut keep_refs = pins.keep_refs.clone();
    keep_refs.sort();
    keep_refs.dedup();

    let mut out = String::new();
    out.push_str("version = 1\n\n[pins]\nkeep = ");
    write_arr(&mut out, &keep);
    out.push('\n');
    out.push_str("keep_refs = ");
    write_arr(&mut out, &keep_refs);
    out.push('\n');

    atomic_write_text(path, out.as_bytes()).map_err(EffectsError::Io)
}

pub(super) fn gc_store_lock(store_dir: &Path) -> Result<GcStoreLock, EffectsError> {
    std::fs::create_dir_all(store_dir)?;
    let lock_path = store_dir.join(".gc.lock");
    ExclusiveLock::acquire(&lock_path)
}

pub(super) type GcDeadSet = (Vec<String>, u64, Vec<(String, u64)>);

pub(super) fn gc_store_dead_set(
    store_dir: &Path,
    live: &std::collections::BTreeSet<String>,
) -> Result<GcDeadSet, EffectsError> {
    let mut dead: Vec<String> = Vec::new();
    let mut dead_bytes: u64 = 0;
    let mut largest: Vec<(String, u64)> = Vec::new();

    for ent in std::fs::read_dir(store_dir)? {
        let ent = ent?;
        let ft = ent.file_type()?;
        if !ft.is_file() {
            continue;
        }
        let name = ent.file_name().to_string_lossy().to_string();
        if gc_vcs::validate_hex_hash(&name).is_err() {
            continue;
        }
        if live.contains(&name) {
            continue;
        }
        let len = ent.metadata()?.len();
        dead_bytes = dead_bytes.saturating_add(len);
        dead.push(name.clone());
        largest.push((name, len));
    }

    dead.sort();
    // Largest list is deterministic: sort by size desc then hash asc.
    largest.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    if largest.len() > 25 {
        largest.truncate(25);
    }
    Ok((dead, dead_bytes, largest))
}
