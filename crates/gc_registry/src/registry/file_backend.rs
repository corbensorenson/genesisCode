fn file_store_path(root: &Path, hash: &str) -> PathBuf {
    root.join("store").join(hash)
}

const FILE_UPLOAD_CHUNK_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileUploadSession {
    hash: String,
    size_bytes: u64,
    chunk_bytes: u64,
    received_chunks: Vec<u64>,
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

fn file_store_put_bytes(root: &Path, hash: &str, bytes: &[u8]) -> Result<(), RegistryError> {
    let got = blake3::hash(bytes).to_hex().to_string();
    if got != hash {
        return Err(RegistryError::Protocol(
            "store/put: hash mismatch".to_string(),
        ));
    }
    let p = file_store_path(root, hash);
    if p.exists() {
        let cur = std::fs::read(&p).map_err(|e| RegistryError::Http(format!("{e}")))?;
        let cur_h = blake3::hash(&cur).to_hex().to_string();
        if cur_h != hash {
            return Err(RegistryError::Protocol("store/put: corruption".to_string()));
        }
        return Ok(());
    }
    file_atomic_write(&p, bytes)
}

fn file_uploads_root(root: &Path) -> PathBuf {
    root.join("uploads")
}

fn file_upload_lock_path(root: &Path) -> PathBuf {
    root.join("uploads.lock")
}

fn file_upload_seq_path(root: &Path) -> PathBuf {
    root.join("uploads.seq")
}

fn file_upload_session_dir(root: &Path, upload_id: &str) -> PathBuf {
    file_uploads_root(root).join(upload_id)
}

fn file_upload_session_meta_path(root: &Path, upload_id: &str) -> PathBuf {
    file_upload_session_dir(root, upload_id).join("session.json")
}

fn file_upload_chunk_path(root: &Path, upload_id: &str, index: u64) -> PathBuf {
    file_upload_session_dir(root, upload_id).join(format!("chunk-{index}.bin"))
}

fn file_upload_lock(root: &Path) -> Result<std::fs::File, RegistryError> {
    let p = file_upload_lock_path(root);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| RegistryError::Http(format!("mkdir: {e}")))?;
    }
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&p)
        .map_err(|e| RegistryError::Http(format!("open uploads lock: {e}")))?;
    #[cfg(not(target_os = "wasi"))]
    {
        f.lock_exclusive()
            .map_err(|e| RegistryError::Http(format!("lock uploads: {e}")))?;
    }
    Ok(f)
}

fn file_next_upload_id(root: &Path, _lk: &mut std::fs::File) -> Result<String, RegistryError> {
    let seq_path = file_upload_seq_path(root);
    let cur = if seq_path.exists() {
        let src = std::fs::read_to_string(&seq_path)
            .map_err(|e| RegistryError::Http(format!("read uploads seq: {e}")))?;
        src.trim()
            .parse::<u64>()
            .map_err(|e| RegistryError::Protocol(format!("uploads seq parse: {e}")))?
    } else {
        0
    };
    let next = cur.saturating_add(1);
    file_atomic_write(&seq_path, next.to_string().as_bytes())?;
    Ok(format!("u_{next}"))
}

fn file_read_upload_session(root: &Path, upload_id: &str) -> Result<FileUploadSession, RegistryError> {
    let meta_path = file_upload_session_meta_path(root, upload_id);
    if !meta_path.exists() {
        return Err(RegistryError::Http("store/upload: status 404".to_string()));
    }
    let src = std::fs::read_to_string(&meta_path)
        .map_err(|e| RegistryError::Http(format!("store/upload/read session: {e}")))?;
    serde_json::from_str(&src)
        .map_err(|e| RegistryError::Protocol(format!("store/upload/session decode: {e}")))
}

fn file_write_upload_session(
    root: &Path,
    upload_id: &str,
    session: &FileUploadSession,
) -> Result<(), RegistryError> {
    let meta_path = file_upload_session_meta_path(root, upload_id);
    let encoded = serde_json::to_vec(session)
        .map_err(|e| RegistryError::Protocol(format!("store/upload/session encode: {e}")))?;
    file_atomic_write(&meta_path, &encoded)
}

fn file_store_upload_start(
    root: &Path,
    hash: &str,
    size_bytes: u64,
) -> Result<StoreUploadStartResp, RegistryError> {
    file_ensure_dirs(root)?;
    std::fs::create_dir_all(file_uploads_root(root))
        .map_err(|e| RegistryError::Http(format!("store/upload/start mkdir: {e}")))?;
    let mut lk = file_upload_lock(root)?;
    let upload_id = file_next_upload_id(root, &mut lk)?;
    let session_dir = file_upload_session_dir(root, &upload_id);
    std::fs::create_dir_all(&session_dir)
        .map_err(|e| RegistryError::Http(format!("store/upload/start session mkdir: {e}")))?;
    let session = FileUploadSession {
        hash: hash.to_string(),
        size_bytes,
        chunk_bytes: FILE_UPLOAD_CHUNK_BYTES,
        received_chunks: Vec::new(),
    };
    file_write_upload_session(root, &upload_id, &session)?;
    Ok(StoreUploadStartResp {
        upload_id,
        chunk_bytes: FILE_UPLOAD_CHUNK_BYTES,
    })
}

fn file_store_upload_chunk(
    root: &Path,
    upload_id: &str,
    index: u64,
    bytes: &[u8],
) -> Result<StoreUploadChunkResp, RegistryError> {
    let mut session = file_read_upload_session(root, upload_id)?;
    if bytes.len() as u64 > session.chunk_bytes {
        return Err(RegistryError::Protocol(
            "store/upload/chunk: exceeds max_chunk_bytes".to_string(),
        ));
    }
    let chunk_path = file_upload_chunk_path(root, upload_id, index);
    file_atomic_write(&chunk_path, bytes)?;
    if !session.received_chunks.contains(&index) {
        session.received_chunks.push(index);
        session.received_chunks.sort_unstable();
    }
    file_write_upload_session(root, upload_id, &session)?;
    Ok(StoreUploadChunkResp {
        ok: true,
        received: bytes.len() as u64,
    })
}

fn file_store_upload_finish(
    root: &Path,
    upload_id: &str,
) -> Result<StoreUploadFinishResp, RegistryError> {
    let mut session = file_read_upload_session(root, upload_id)?;
    session.received_chunks.sort_unstable();
    for (expected, idx) in session.received_chunks.iter().enumerate() {
        if *idx != expected as u64 {
            return Err(RegistryError::Protocol(
                "store/upload/finish: missing chunk index".to_string(),
            ));
        }
    }
    let mut payload = Vec::new();
    for idx in &session.received_chunks {
        let chunk = std::fs::read(file_upload_chunk_path(root, upload_id, *idx))
            .map_err(|_| RegistryError::Protocol("store/upload/finish: missing chunk".to_string()))?;
        payload.extend_from_slice(&chunk);
    }
    if payload.len() as u64 != session.size_bytes {
        return Err(RegistryError::Protocol(
            "store/upload/finish: size mismatch".to_string(),
        ));
    }
    let got = blake3::hash(&payload).to_hex().to_string();
    if got != session.hash {
        return Err(RegistryError::Protocol(
            "store/upload/finish: hash mismatch".to_string(),
        ));
    }
    file_store_put_bytes(root, &session.hash, &payload)?;
    std::fs::remove_dir_all(file_upload_session_dir(root, upload_id))
        .map_err(|e| RegistryError::Http(format!("store/upload/finish cleanup: {e}")))?;
    Ok(StoreUploadFinishResp { ok: true })
}

fn file_store_upload_status(root: &Path, upload_id: &str) -> Result<StoreUploadStatusResp, RegistryError> {
    let mut session = file_read_upload_session(root, upload_id)?;
    session.received_chunks.sort_unstable();
    Ok(StoreUploadStatusResp {
        received_chunks: session.received_chunks,
    })
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
    let mut requirements_trace_terms: Vec<Term> = Vec::new();
    let mut tool_qualification_terms: Vec<Term> = Vec::new();
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
        let norm_kind = gc_vcs::PolicyClass::normalize_evidence_kind(&ev.kind);
        if norm_kind == ":requirements-trace" {
            requirements_trace_terms.push(ev_t.clone());
        } else if norm_kind == ":tool-qualification" {
            tool_qualification_terms.push(ev_t.clone());
        }
        evidence_kinds.insert(norm_kind);
    }
    let missing_kinds = class.missing_required_evidence_kinds(&commit.obligations, &evidence_kinds);
    if !missing_kinds.is_empty() {
        return Err(RegistryError::Http("refs/set: status 403".to_string()));
    }
    let required_kinds = class.required_evidence_kind_set(&commit.obligations);
    if required_kinds.contains(":requirements-trace") {
        if requirements_trace_terms.is_empty() {
            return Err(RegistryError::Http("refs/set: status 403".to_string()));
        }
        let ctx = gc_vcs::RequirementsTraceGateContext {
            commit_hash: commit_h,
            snapshot_hash: &commit.result,
            policy_hash: Some(policy_h),
            commit_obligations: &commit.obligations,
            observed_evidence_kinds: &evidence_kinds,
        };
        for t in &requirements_trace_terms {
            if gc_vcs::validate_requirements_trace_evidence(t, &ctx).is_err() {
                return Err(RegistryError::Http("refs/set: status 403".to_string()));
            }
        }
    }
    if required_kinds.contains(":tool-qualification") {
        if tool_qualification_terms.is_empty() {
            return Err(RegistryError::Http("refs/set: status 403".to_string()));
        }
        let ctx = gc_vcs::ToolQualificationGateContext {
            commit_hash: commit_h,
            snapshot_hash: &commit.result,
            policy_hash: Some(policy_h),
        };
        for t in &tool_qualification_terms {
            if gc_vcs::validate_tool_qualification_evidence(t, &ctx).is_err() {
                return Err(RegistryError::Http("refs/set: status 403".to_string()));
            }
        }
    }

    if class.require_signatures {
        let signing_h = gc_vcs::commit_signing_hash(&commit_term).map_err(|e| {
            RegistryError::Protocol(format!("refs/set: bad commit signing hash: {e}"))
        })?;
        let mut valid: u64 = 0;
        let mut seen_pks: std::collections::BTreeSet<Vec<u8>> = std::collections::BTreeSet::new();
        let mut role_signers: std::collections::BTreeMap<String, std::collections::BTreeSet<Vec<u8>>> =
            std::collections::BTreeMap::new();
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
            let pk_vec = att.pk.to_vec();
            if gc_vcs::verify_commit_attestation(&att, &signing_h, &class.allowed_public_keys).is_ok()
            {
                if seen_pks.insert(pk_vec.clone()) {
                    valid = valid.saturating_add(1);
                }
                if let Some(role) = att.role.as_deref() {
                    let norm = gc_vcs::PolicyClass::normalize_attestation_role(role);
                    if norm != ":" {
                        role_signers.entry(norm).or_default().insert(pk_vec);
                    }
                }
            }
        }
        if valid < class.min_signatures {
            return Err(RegistryError::Http("refs/set: status 403".to_string()));
        }
        for role in &class.required_attestation_roles {
            if role_signers.get(role).map_or(0, |s| s.len()) == 0 {
                return Err(RegistryError::Http("refs/set: status 403".to_string()));
            }
        }
        for (role, min) in &class.role_min_signatures {
            if role_signers.get(role).map_or(0, |s| s.len()) < *min as usize {
                return Err(RegistryError::Http("refs/set: status 403".to_string()));
            }
        }
        for (left, right) in &class.independent_role_pairs {
            let left_set = role_signers.get(left);
            let right_set = role_signers.get(right);
            if left_set.map_or(0, |s| s.len()) == 0 || right_set.map_or(0, |s| s.len()) == 0 {
                return Err(RegistryError::Http("refs/set: status 403".to_string()));
            }
            if let (Some(a), Some(b)) = (left_set, right_set)
                && a.iter().any(|pk| b.contains(pk))
            {
                return Err(RegistryError::Http("refs/set: status 403".to_string()));
            }
        }
    }

    Ok(())
}
