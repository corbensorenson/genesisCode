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
            .apply(&mut helper_ctx, Value::data(t.clone()))
            .and_then(|f| f.apply(&mut helper_ctx, Value::data(opts_term)))
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
    lock_authority: Option<&mut PkgLockReadAuthority>,
    error_tok: SealId,
    op: &str,
) -> Result<(Vec<Term>, Term, Term), Value> {
    let mut ref_entries: Vec<Term> = Vec::new();
    // Pinned references must resolve against the snapshot even when ordinary
    // refs are excluded from the root set; GenesisCode applies include_refs.
    if let Some(rdb) = refs {
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
    if include_lock {
        let lock_path = sandbox_path_allow_missing(base_dir, lock_s, false).map_err(|error| {
            mk_error(error_tok, "core/gc/bad-lock", error.to_string(), Some(op))
        })?;
        if lock_path.exists() {
            let Some(lock_authority) = lock_authority else {
                return Err(mk_error(
                    error_tok,
                    "core/gc/lock-authority-unavailable",
                    "GC lock roots require the artifact-loaded GenesisCode lock model authority"
                        .to_string(),
                    Some(op),
                ));
            };
            let bytes = runner_cap_pkg_low::read_bounded_lock(&lock_path)
                .map_err(|message| mk_error(error_tok, "core/gc/bad-lock", message, Some(op)))?;
            match lock_authority.read_model_toml(&bytes) {
                Ok(PkgLockModelDecision::Lock(lk)) => {
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
                Ok(PkgLockModelDecision::Error { message, .. }) => {
                    return Err(mk_error(error_tok, "core/gc/bad-lock", message, Some(op)));
                }
                Err(error) => {
                    return Err(mk_error(
                        error_tok,
                        "core/gc/lock-authority-error",
                        error.to_string(),
                        Some(op),
                    ));
                }
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

    let pins_document = gc_pins_document(base_dir, pins_s)
        .map_err(|error| mk_error(error_tok, "core/gc/bad-pins", error, Some(op)))?;

    Ok((ref_entries, lock_info, pins_document))
}

// -----------------------------------------------------------------------------
// GC helpers (pins + store lock + store scan)
// -----------------------------------------------------------------------------

pub(super) fn gc_pins_document(base_dir: &Path, pins_path: &str) -> Result<Term, String> {
    let path = sandbox_path_allow_missing(base_dir, pins_path, false).map_err(|e| e.to_string())?;
    gc_pins_document_at(&path)
}

pub(super) fn gc_pins_document_at(path: &Path) -> Result<Term, String> {
    use std::io::Read;

    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Term::Nil),
        Err(error) => return Err(format!("pins open failed: {error}")),
    };
    let metadata = file
        .metadata()
        .map_err(|error| format!("pins descriptor metadata failed: {error}"))?;
    if !metadata.file_type().is_file() {
        return Err("pins path must identify a regular file".to_string());
    }
    const MAX_PINS_BYTES: u64 = 4 * 1024 * 1024;
    if metadata.len() > MAX_PINS_BYTES {
        return Err(format!("pins document exceeds {MAX_PINS_BYTES} bytes"));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_PINS_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("pins read failed: {error}"))?;
    if bytes.len() as u64 > MAX_PINS_BYTES {
        return Err(format!("pins document exceeds {MAX_PINS_BYTES} bytes"));
    }
    let source = std::str::from_utf8(&bytes).map_err(|_| "pins file is not utf-8".to_string())?;
    let document: toml::Value =
        toml::from_str(source).map_err(|error| format!("pins toml parse: {error}"))?;
    Ok(gc_toml_to_term(document))
}

fn gc_toml_to_term(value: toml::Value) -> Term {
    match value {
        toml::Value::String(value) => Term::Str(value),
        toml::Value::Integer(value) => Term::Int(value.into()),
        toml::Value::Float(value) => Term::Str(value.to_string()),
        toml::Value::Boolean(value) => Term::Bool(value),
        toml::Value::Datetime(value) => Term::Str(value.to_string()),
        toml::Value::Array(values) => {
            Term::Vector(values.into_iter().map(gc_toml_to_term).collect())
        }
        toml::Value::Table(values) => Term::Map(
            values
                .into_iter()
                .map(|(key, value)| (TermOrdKey(Term::Str(key)), gc_toml_to_term(value)))
                .collect(),
        ),
    }
}

pub(super) fn gc_store_lock(store_dir: &Path) -> Result<GcStoreLock, EffectsError> {
    std::fs::create_dir_all(store_dir)?;
    let lock_path = store_dir.join(".gc.lock");
    ExclusiveLock::acquire(&lock_path)
}

pub(super) fn gc_path_lock(path: &Path) -> Result<GcStoreLock, EffectsError> {
    let mut lock_name = path.as_os_str().to_os_string();
    lock_name.push(".lock");
    ExclusiveLock::acquire(Path::new(&lock_name))
}

pub(super) fn gc_store_inventory(
    store: &ArtifactStore,
) -> Result<Vec<(String, u64)>, EffectsError> {
    let store_dir = store.root_dir();
    let mut inventory = Vec::new();
    for entry in std::fs::read_dir(store_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let Ok(hash) = entry.file_name().into_string() else {
            continue;
        };
        if !is_canonical_gc_hash(&hash) {
            continue;
        }
        store.verify_hex(&hash)?;
        inventory.push((hash, entry.metadata()?.len()));
    }
    inventory.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(inventory)
}

pub(super) fn gc_quarantine_inventory(
    directory: &Path,
    now: std::time::SystemTime,
) -> Result<Vec<(String, u64)>, EffectsError> {
    let mut inventory = Vec::new();
    if !directory.exists() {
        return Ok(inventory);
    }
    let quarantine_store = ArtifactStore::open_with_integrity_cache(directory, false)?;
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let Ok(hash) = entry.file_name().into_string() else {
            continue;
        };
        if !is_canonical_gc_hash(&hash) {
            continue;
        }
        quarantine_store.verify_hex(&hash)?;
        let modified = entry.metadata()?.modified()?;
        let age = now.duration_since(modified).unwrap_or_default().as_secs();
        inventory.push((hash, age));
    }
    inventory.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(inventory)
}

fn is_canonical_gc_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

pub(super) fn gc_closure_local(
    store: &ArtifactStore,
    authority: &mut GcAuthority,
    root: &str,
    depth: u64,
    out: &mut std::collections::BTreeSet<String>,
    error_tok: SealId,
    op: &str,
) -> Result<(), Value> {
    use std::collections::{HashSet, VecDeque};

    let mut queue = VecDeque::from([(root.to_string(), depth)]);
    let mut seen = HashSet::new();
    while let Some((hash, depth_left)) = queue.pop_front() {
        if !seen.insert(hash.clone()) {
            continue;
        }
        if seen.len() > 50_000 {
            return Err(mk_error(
                error_tok,
                "core/gc/too-many-objects",
                "closure exceeded 50k objects".to_string(),
                Some(op),
            ));
        }
        if store.verify_hex(&hash).is_err() {
            return Err(mk_error(
                error_tok,
                "core/store/corruption",
                format!("artifact is absent or corrupt: {hash}"),
                Some(op),
            ));
        }
        out.insert(hash.clone());
        let Ok(artifact) = store_get_term(store, &hash) else {
            continue;
        };
        let edges = authority
            .artifact_edges(artifact, true, true, depth_left > 0)
            .map_err(|error| {
                mk_error(
                    error_tok,
                    "core/gc/authority-error",
                    error.to_string(),
                    Some(op),
                )
            })?;
        for next in edges.refs {
            queue.push_back((next, depth_left));
        }
        if depth_left > 0 {
            for parent in edges.parents {
                queue.push_back((parent, depth_left - 1));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gc_pins_reader_rejects_oversized_regular_files() {
        let directory = tempfile::tempdir().expect("temporary GC workspace");
        let path = directory.path().join("pins.toml");
        let file = std::fs::File::create(&path).expect("create pins fixture");
        file.set_len(4 * 1024 * 1024 + 1)
            .expect("size pins fixture");

        assert_eq!(
            gc_pins_document_at(&path).expect_err("oversized pins must fail closed"),
            "pins document exceeds 4194304 bytes"
        );
    }

    #[cfg(unix)]
    #[test]
    fn gc_pins_reader_rejects_fifo_without_blocking() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let directory = tempfile::tempdir().expect("temporary GC workspace");
        let path = directory.path().join("pins.fifo");
        let c_path = CString::new(path.as_os_str().as_bytes()).expect("fifo path without nul");
        // SAFETY: c_path is a valid NUL-terminated path and no pointer escapes this call.
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);

        assert_eq!(
            gc_pins_document_at(&path).expect_err("FIFO pins must fail closed"),
            "pins path must identify a regular file"
        );
    }

    #[test]
    fn gc_lock_authority_fails_closed_before_store_mutation_when_missing() {
        let directory = tempfile::tempdir().expect("temporary GC workspace");
        std::fs::write(
            directory.path().join("genesis.lock"),
            "version = 2\nworkspace = \"fixture\"\n",
        )
        .expect("write lock fixture");
        let base_dir = directory
            .path()
            .canonicalize()
            .expect("canonical temporary GC workspace");
        let sentinel = directory.path().join("unrelated-store-object");
        std::fs::write(&sentinel, b"must remain").expect("write mutation sentinel");

        let mut context = EvalCtx::new();
        let error_token = build_prelude(&mut context).protocol.error;
        let error = gc_build_sources(
            None,
            &base_dir,
            "genesis.lock",
            ".genesis/pins.toml",
            true,
            None,
            error_token,
            "core/gc-low::run",
        )
        .expect_err("existing lock roots must require artifact authority");

        let Value::Sealed { token, payload } = error else {
            panic!("expected sealed boundary error");
        };
        assert_eq!(token, error_token);
        let Some(Term::Map(fields)) = payload.as_data() else {
            panic!("expected sealed boundary error map");
        };
        assert_eq!(
            fields.get(&TermOrdKey(Term::symbol(":error/code"))),
            Some(&Term::Str("core/gc/lock-authority-unavailable".to_string()))
        );
        assert_eq!(
            fields.get(&TermOrdKey(Term::symbol(":error/op"))),
            Some(&Term::symbol("core/gc-low::run"))
        );
        assert!(
            sentinel.exists(),
            "authority failure must not mutate storage"
        );
    }

    #[test]
    fn gc_lock_path_failure_is_sealed_before_store_mutation() {
        let directory = tempfile::tempdir().expect("temporary GC workspace");
        let base_dir = directory
            .path()
            .canonicalize()
            .expect("canonical temporary GC workspace");
        let sentinel = directory.path().join("unrelated-store-object");
        std::fs::write(&sentinel, b"must remain").expect("write mutation sentinel");

        let mut context = EvalCtx::new();
        let error_token = build_prelude(&mut context).protocol.error;
        let error = gc_build_sources(
            None,
            &base_dir,
            "../outside/genesis.lock",
            ".genesis/pins.toml",
            true,
            None,
            error_token,
            "core/gc-low::run",
        )
        .expect_err("invalid lock paths must not be treated as absent locks");

        let Value::Sealed { token, payload } = error else {
            panic!("expected sealed boundary error");
        };
        assert_eq!(token, error_token);
        let Some(Term::Map(fields)) = payload.as_data() else {
            panic!("expected sealed boundary error map");
        };
        assert_eq!(
            fields.get(&TermOrdKey(Term::symbol(":error/code"))),
            Some(&Term::Str("core/gc/bad-lock".to_string()))
        );
        assert!(
            sentinel.exists(),
            "lock-path failure must not mutate storage"
        );
    }
}
