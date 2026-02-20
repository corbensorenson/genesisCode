fn file_store_path(root: &Path, hash: &str) -> PathBuf {
    root.join("store").join(hash)
}

fn file_refs_path(root: &Path) -> PathBuf {
    root.join("refs.gc")
}

fn file_refs_lock_path(root: &Path) -> PathBuf {
    root.join("refs.lock")
}

fn file_ensure_dirs(root: &Path) -> Result<(), RegistryError> {
    std::fs::create_dir_all(root.join("store"))
        .map_err(|e| RegistryError::Http(format!("file registry mkdir: {e}")))?;
    Ok(())
}

fn file_atomic_write(path: &Path, bytes: &[u8]) -> Result<(), RegistryError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| RegistryError::Http(format!("mkdir: {e}")))?;
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&tmp)
        .map_err(|e| RegistryError::Http(format!("write tmp: {e}")))?;
    f.write_all(bytes)
        .map_err(|e| RegistryError::Http(format!("write tmp: {e}")))?;
    f.sync_all()
        .map_err(|e| RegistryError::Http(format!("fsync tmp: {e}")))?;
    std::fs::rename(&tmp, path).map_err(|e| RegistryError::Http(format!("rename: {e}")))?;
    #[cfg(unix)]
    {
        if let Some(parent) = path.parent() {
            let dir = std::fs::File::open(parent)
                .map_err(|e| RegistryError::Http(format!("open dir: {e}")))?;
            dir.sync_all()
                .map_err(|e| RegistryError::Http(format!("fsync dir: {e}")))?;
        }
    }
    Ok(())
}

fn file_refs_lock(root: &Path) -> Result<std::fs::File, RegistryError> {
    let p = file_refs_lock_path(root);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| RegistryError::Http(format!("mkdir: {e}")))?;
    }
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&p)
        .map_err(|e| RegistryError::Http(format!("open refs lock: {e}")))?;
    #[cfg(not(target_os = "wasi"))]
    {
        f.lock_exclusive()
            .map_err(|e| RegistryError::Http(format!("lock refs: {e}")))?;
    }
    Ok(f)
}

fn file_load_refs_locked(root: &Path) -> Result<BTreeMap<String, String>, RegistryError> {
    let p = file_refs_path(root);
    if !p.exists() {
        return Ok(BTreeMap::new());
    }
    let s = std::fs::read_to_string(&p).map_err(|e| RegistryError::Http(format!("{e}")))?;
    let t = gc_coreform::parse_term(&s)
        .map_err(|e| RegistryError::Protocol(format!("refs db parse: {e}")))?;
    let Term::Map(m) = t else {
        return Err(RegistryError::Protocol("refs db: expected map".to_string()));
    };
    let v = m.get(&TermOrdKey(Term::symbol(":v")));
    if !matches!(v, Some(Term::Int(i)) if i == &1.into()) {
        return Err(RegistryError::Protocol(
            "refs db: wrong or missing :v".to_string(),
        ));
    }
    let kind = m.get(&TermOrdKey(Term::symbol(":kind")));
    if !matches!(kind, Some(Term::Str(s)) if s == "genesis/refs-db-v0.1") {
        return Err(RegistryError::Protocol(
            "refs db: wrong or missing :kind".to_string(),
        ));
    }
    let Some(Term::Map(refs)) = m.get(&TermOrdKey(Term::symbol(":refs"))) else {
        return Err(RegistryError::Protocol(
            "refs db: missing :refs map".to_string(),
        ));
    };
    let mut out = BTreeMap::new();
    for (k, v) in refs {
        let Term::Str(name) = &k.0 else {
            return Err(RegistryError::Protocol(
                "refs db: :refs keys must be strings".to_string(),
            ));
        };
        let Term::Str(hex) = v else {
            return Err(RegistryError::Protocol(
                "refs db: :refs values must be strings".to_string(),
            ));
        };
        out.insert(name.clone(), hex.clone());
    }
    Ok(out)
}

fn file_write_refs_locked(
    root: &Path,
    db: &BTreeMap<String, String>,
    _lk: &mut std::fs::File,
) -> Result<(), RegistryError> {
    let mut refs = BTreeMap::new();
    for (k, v) in db {
        refs.insert(TermOrdKey(Term::Str(k.clone())), Term::Str(v.clone()));
    }
    let t = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/refs-db-v0.1".to_string()),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (TermOrdKey(Term::symbol(":refs")), Term::Map(refs)),
        ]
        .into_iter()
        .collect(),
    );
    let s = gc_coreform::print_term(&t) + "\n";
    file_atomic_write(&file_refs_path(root), s.as_bytes())
}

fn file_gate_refs_set(
    root: &Path,
    name: &str,
    commit_h: &str,
    policy_h: &str,
) -> Result<(), RegistryError> {
    // Resolve policy term from remote store.
    let pol_bytes = std::fs::read(file_store_path(root, policy_h))
        .map_err(|_| RegistryError::Http("refs/set: status 403".to_string()))?;
    if blake3::hash(&pol_bytes).to_hex().to_string() != policy_h {
        return Err(RegistryError::Protocol(
            "refs/set: policy corruption".to_string(),
        ));
    }
    let pol_s = String::from_utf8(pol_bytes)
        .map_err(|_| RegistryError::Protocol("refs/set: policy not utf8".to_string()))?;
    let pol_term = gc_coreform::parse_term(&pol_s)
        .map_err(|e| RegistryError::Protocol(format!("refs/set: bad policy term: {e}")))?;
    let pol = gc_vcs::Policy::from_term(&pol_term)
        .map_err(|e| RegistryError::Protocol(format!("refs/set: bad policy schema: {e}")))?;
    if pol.is_frozen_ref(name) {
        return Err(RegistryError::Http("refs/set: status 403".to_string()));
    }
    let class = pol
        .class_for_ref(name)
        .ok_or_else(|| RegistryError::Http("refs/set: status 403".to_string()))?;

    let commit_bytes = std::fs::read(file_store_path(root, commit_h))
        .map_err(|_| RegistryError::Http("refs/set: status 403".to_string()))?;
    if blake3::hash(&commit_bytes).to_hex().to_string() != commit_h {
        return Err(RegistryError::Protocol(
            "refs/set: commit corruption".to_string(),
        ));
    }
    let commit_s = String::from_utf8(commit_bytes)
        .map_err(|_| RegistryError::Protocol("refs/set: commit not utf8".to_string()))?;
    let commit_term = gc_coreform::parse_term(&commit_s)
        .map_err(|e| RegistryError::Protocol(format!("refs/set: bad commit term: {e}")))?;
    let commit = gc_vcs::Commit::from_term(&commit_term)
        .map_err(|e| RegistryError::Protocol(format!("refs/set: bad commit schema: {e}")))?;

    // Ensure pointer targets exist (minimum server sanity).
    if let Some(b) = commit.base.as_ref()
        && !file_store_path(root, b).exists()
    {
        return Err(RegistryError::Http("refs/set: status 403".to_string()));
    }
    if !file_store_path(root, &commit.patch).exists()
        || !file_store_path(root, &commit.result).exists()
    {
        return Err(RegistryError::Http("refs/set: status 403".to_string()));
    }

    for req in &class.required_obligations {
        if !commit.obligations.iter().any(|o| o == req) {
            return Err(RegistryError::Http("refs/set: status 403".to_string()));
        }
    }
    if !class.required_obligations.is_empty() && commit.evidence.is_empty() {
        return Err(RegistryError::Http("refs/set: status 403".to_string()));
    }
    let mut evidence_kinds: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for ev_h in &commit.evidence {
        let ev_bytes = std::fs::read(file_store_path(root, ev_h))
            .map_err(|_| RegistryError::Http("refs/set: status 403".to_string()))?;
        if blake3::hash(&ev_bytes).to_hex().to_string() != *ev_h {
            return Err(RegistryError::Protocol(
                "refs/set: evidence corruption".to_string(),
            ));
        }
        let ev_s = String::from_utf8(ev_bytes)
            .map_err(|_| RegistryError::Protocol("refs/set: evidence not utf8".to_string()))?;
        let ev_t = gc_coreform::parse_term(&ev_s)
            .map_err(|e| RegistryError::Protocol(format!("refs/set: bad evidence term: {e}")))?;
        let ev = gc_vcs::Evidence::from_term(&ev_t)
            .map_err(|e| RegistryError::Protocol(format!("refs/set: bad evidence schema: {e}")))?;
        evidence_kinds.insert(gc_vcs::PolicyClass::normalize_evidence_kind(&ev.kind));
    }
    let missing_kinds = class.missing_required_evidence_kinds(&commit.obligations, &evidence_kinds);
    if !missing_kinds.is_empty() {
        return Err(RegistryError::Http("refs/set: status 403".to_string()));
    }

    if class.require_signatures {
        let signing_h = gc_vcs::commit_signing_hash(&commit_term).map_err(|e| {
            RegistryError::Protocol(format!("refs/set: bad commit signing hash: {e}"))
        })?;
        let mut valid: u64 = 0;
        let mut seen_pks: std::collections::BTreeSet<Vec<u8>> = std::collections::BTreeSet::new();
        for at_h in &commit.attestations {
            let at_bytes = std::fs::read(file_store_path(root, at_h))
                .map_err(|_| RegistryError::Http("refs/set: status 403".to_string()))?;
            if blake3::hash(&at_bytes).to_hex().to_string() != *at_h {
                return Err(RegistryError::Protocol(
                    "refs/set: attestation corruption".to_string(),
                ));
            }
            let at_s = String::from_utf8(at_bytes).map_err(|_| {
                RegistryError::Protocol("refs/set: attestation not utf8".to_string())
            })?;
            let at_t = gc_coreform::parse_term(&at_s).map_err(|e| {
                RegistryError::Protocol(format!("refs/set: bad attestation term: {e}"))
            })?;
            let att = gc_vcs::Attestation::from_term(&at_t).map_err(|e| {
                RegistryError::Protocol(format!("refs/set: bad attestation schema: {e}"))
            })?;
            if !seen_pks.insert(att.pk.to_vec()) {
                continue;
            }
            if gc_vcs::verify_commit_attestation(&att, &signing_h, &class.allowed_public_keys)
                .is_ok()
            {
                valid = valid.saturating_add(1);
            }
        }
        if valid < class.min_signatures {
            return Err(RegistryError::Http("refs/set: status 403".to_string()));
        }
    }

    Ok(())
}
