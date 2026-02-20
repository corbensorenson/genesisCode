use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use gc_coreform::{Term, TermOrdKey, parse_term, print_term};
use gc_kernel::{EvalCtx, Value, eval_module, value_hash};
use gc_prelude::build_prelude;

use gc_effects::{CapsPolicy, Decision, EffectLog, replay, run};

#[derive(Debug, Default)]
struct RegistryState {
    store: BTreeMap<String, Vec<u8>>,
    refs: BTreeMap<String, String>,
    uploads: BTreeMap<String, UploadSession>,
    upload_started: u64,
    upload_chunk_calls: u64,
    upload_finished: u64,
}

#[derive(Debug, Clone)]
struct UploadSession {
    hash: String,
    size_bytes: u64,
    chunk_bytes: u64,
    chunks: BTreeMap<u64, Vec<u8>>,
}

#[derive(Debug)]
struct MemRegistry {
    st: Mutex<RegistryState>,
    required_bearer: Option<String>,
    required_basic: Option<(String, String)>,
    require_mtls: bool,
    auth_audit: Mutex<Vec<AuthAudit>>,
    max_chunk_bytes: u64,
    next_upload_id: AtomicU64,
}

#[derive(Debug, Clone, Default)]
struct AuthAudit {
    bearer_present: bool,
    basic_username: Option<String>,
    basic_password_present: bool,
    mtls_ca_present: bool,
    mtls_identity_present: bool,
}

fn hash_bytes_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

impl MemRegistry {
    fn new() -> Self {
        Self::new_with_max_chunk_bytes(4_194_304)
    }

    fn new_with_max_chunk_bytes(max_chunk_bytes: u64) -> Self {
        Self {
            st: Mutex::new(RegistryState::default()),
            required_bearer: None,
            required_basic: None,
            require_mtls: false,
            auth_audit: Mutex::new(Vec::new()),
            max_chunk_bytes,
            next_upload_id: AtomicU64::new(1),
        }
    }

    fn new_with_required_bearer(token: &str) -> Self {
        Self {
            st: Mutex::new(RegistryState::default()),
            required_bearer: Some(token.to_string()),
            required_basic: None,
            require_mtls: false,
            auth_audit: Mutex::new(Vec::new()),
            max_chunk_bytes: 4_194_304,
            next_upload_id: AtomicU64::new(1),
        }
    }

    fn new_with_required_basic(username: &str, password: &str) -> Self {
        Self {
            st: Mutex::new(RegistryState::default()),
            required_bearer: None,
            required_basic: Some((username.to_string(), password.to_string())),
            require_mtls: false,
            auth_audit: Mutex::new(Vec::new()),
            max_chunk_bytes: 4_194_304,
            next_upload_id: AtomicU64::new(1),
        }
    }

    fn new_with_required_mtls() -> Self {
        Self {
            st: Mutex::new(RegistryState::default()),
            required_bearer: None,
            required_basic: None,
            require_mtls: true,
            auth_audit: Mutex::new(Vec::new()),
            max_chunk_bytes: 4_194_304,
            next_upload_id: AtomicU64::new(1),
        }
    }

    fn put_artifact(&self, bytes: &[u8]) -> String {
        let hex = hash_bytes_hex(bytes);
        let mut g = self.st.lock().expect("lock");
        g.store.insert(hex.clone(), bytes.to_vec());
        hex
    }

    fn has(&self, hex: &str) -> bool {
        let g = self.st.lock().expect("lock");
        g.store.contains_key(hex)
    }

    fn ref_get(&self, name: &str) -> Option<String> {
        let g = self.st.lock().expect("lock");
        g.refs.get(name).cloned()
    }

    fn auth_audit(&self) -> Vec<AuthAudit> {
        self.auth_audit.lock().expect("lock").clone()
    }

    fn upload_counts(&self) -> (u64, u64, u64) {
        let g = self.st.lock().expect("lock");
        (g.upload_started, g.upload_chunk_calls, g.upload_finished)
    }
}

impl gc_registry::InProcRegistry for MemRegistry {
    fn authorize(
        &self,
        auth: &gc_registry::RegistryAuth,
    ) -> Result<(), gc_registry::RegistryError> {
        {
            let mut g = self.auth_audit.lock().expect("lock");
            g.push(AuthAudit {
                bearer_present: auth.bearer_token.is_some(),
                basic_username: auth.basic_username.clone(),
                basic_password_present: auth.basic_password.is_some(),
                mtls_ca_present: auth.mtls_ca_pem.is_some(),
                mtls_identity_present: auth.mtls_identity_pem.is_some(),
            });
        }
        if let Some(expected) = &self.required_bearer {
            match auth.bearer_token.as_deref() {
                Some(got) if got == expected => {}
                _ => {
                    return Err(gc_registry::RegistryError::Auth(
                        "missing or invalid bearer token".to_string(),
                    ));
                }
            }
        }
        if let Some((expected_user, expected_password)) = &self.required_basic {
            match (
                auth.basic_username.as_deref(),
                auth.basic_password.as_deref(),
            ) {
                (Some(user), Some(password))
                    if user == expected_user && password == expected_password => {}
                _ => {
                    return Err(gc_registry::RegistryError::Auth(
                        "missing or invalid basic auth credentials".to_string(),
                    ));
                }
            }
        }
        if self.require_mtls && (auth.mtls_ca_pem.is_none() || auth.mtls_identity_pem.is_none()) {
            return Err(gc_registry::RegistryError::Auth(
                "missing mTLS identity materials".to_string(),
            ));
        }
        Ok(())
    }

    fn ping(&self) -> Result<gc_registry::PingResp, gc_registry::RegistryError> {
        Ok(gc_registry::PingResp {
            ok: true,
            version: "0.1".to_string(),
            hash: "blake3-256".to_string(),
            max_chunk_bytes: Some(self.max_chunk_bytes),
        })
    }

    fn store_has(
        &self,
        hashes: &[String],
    ) -> Result<BTreeMap<String, bool>, gc_registry::RegistryError> {
        let g = self.st.lock().expect("lock");
        let mut out = BTreeMap::new();
        for h in hashes {
            out.insert(h.clone(), g.store.contains_key(h));
        }
        Ok(out)
    }

    fn store_get(&self, hash: &str) -> Result<Vec<u8>, gc_registry::RegistryError> {
        let g = self.st.lock().expect("lock");
        g.store
            .get(hash)
            .cloned()
            .ok_or_else(|| gc_registry::RegistryError::Http("store/get: status 404".to_string()))
    }

    fn store_put(&self, hash: &str, bytes: &[u8]) -> Result<(), gc_registry::RegistryError> {
        let got = hash_bytes_hex(bytes);
        if got != hash {
            return Err(gc_registry::RegistryError::Protocol(
                "store/put: hash mismatch".to_string(),
            ));
        }
        let mut g = self.st.lock().expect("lock");
        g.store.entry(hash.to_string()).or_insert(bytes.to_vec());
        Ok(())
    }

    fn store_upload_start(
        &self,
        hash: &str,
        size_bytes: u64,
    ) -> Result<gc_registry::StoreUploadStartResp, gc_registry::RegistryError> {
        let upload_id = format!("u_{}", self.next_upload_id.fetch_add(1, Ordering::Relaxed));
        let mut g = self.st.lock().expect("lock");
        g.uploads.insert(
            upload_id.clone(),
            UploadSession {
                hash: hash.to_string(),
                size_bytes,
                chunk_bytes: self.max_chunk_bytes,
                chunks: BTreeMap::new(),
            },
        );
        g.upload_started = g.upload_started.saturating_add(1);
        Ok(gc_registry::StoreUploadStartResp {
            upload_id,
            chunk_bytes: self.max_chunk_bytes,
        })
    }

    fn store_upload_chunk(
        &self,
        upload_id: &str,
        index: u64,
        bytes: &[u8],
    ) -> Result<gc_registry::StoreUploadChunkResp, gc_registry::RegistryError> {
        let mut g = self.st.lock().expect("lock");
        let session = g.uploads.get_mut(upload_id).ok_or_else(|| {
            gc_registry::RegistryError::Http("store/upload/chunk: status 404".to_string())
        })?;
        if bytes.len() as u64 > session.chunk_bytes {
            return Err(gc_registry::RegistryError::Protocol(
                "store/upload/chunk: chunk exceeds advertised chunk_bytes".to_string(),
            ));
        }
        session.chunks.insert(index, bytes.to_vec());
        g.upload_chunk_calls = g.upload_chunk_calls.saturating_add(1);
        Ok(gc_registry::StoreUploadChunkResp {
            ok: true,
            received: bytes.len() as u64,
        })
    }

    fn store_upload_finish(
        &self,
        upload_id: &str,
    ) -> Result<gc_registry::StoreUploadFinishResp, gc_registry::RegistryError> {
        let mut g = self.st.lock().expect("lock");
        let session = g.uploads.remove(upload_id).ok_or_else(|| {
            gc_registry::RegistryError::Http("store/upload/finish: status 404".to_string())
        })?;
        let mut keys: Vec<u64> = session.chunks.keys().copied().collect();
        keys.sort_unstable();
        for (i, k) in keys.iter().enumerate() {
            if *k != i as u64 {
                return Err(gc_registry::RegistryError::Protocol(
                    "store/upload/finish: missing chunk index".to_string(),
                ));
            }
        }
        let mut assembled = Vec::new();
        for i in 0..keys.len() as u64 {
            let chunk = session.chunks.get(&i).expect("chunk exists");
            assembled.extend_from_slice(chunk);
        }
        if assembled.len() as u64 != session.size_bytes {
            return Err(gc_registry::RegistryError::Protocol(
                "store/upload/finish: size mismatch".to_string(),
            ));
        }
        let got = hash_bytes_hex(&assembled);
        if got != session.hash {
            return Err(gc_registry::RegistryError::Protocol(
                "store/upload/finish: hash mismatch".to_string(),
            ));
        }
        g.store.entry(session.hash).or_insert(assembled);
        g.upload_finished = g.upload_finished.saturating_add(1);
        Ok(gc_registry::StoreUploadFinishResp { ok: true })
    }

    fn store_upload_status(
        &self,
        upload_id: &str,
    ) -> Result<gc_registry::StoreUploadStatusResp, gc_registry::RegistryError> {
        let g = self.st.lock().expect("lock");
        let session = g.uploads.get(upload_id).ok_or_else(|| {
            gc_registry::RegistryError::Http("store/upload/status: status 404".to_string())
        })?;
        let mut received_chunks: Vec<u64> = session.chunks.keys().copied().collect();
        received_chunks.sort_unstable();
        Ok(gc_registry::StoreUploadStatusResp { received_chunks })
    }

    fn refs_get(&self, name: &str) -> Result<Option<String>, gc_registry::RegistryError> {
        let g = self.st.lock().expect("lock");
        Ok(g.refs.get(name).cloned())
    }

    fn refs_list(
        &self,
        prefix: Option<&str>,
    ) -> Result<Vec<gc_registry::RefsListEntry>, gc_registry::RegistryError> {
        let g = self.st.lock().expect("lock");
        let mut out = Vec::new();
        for (name, hash) in &g.refs {
            if let Some(p) = prefix
                && !name.starts_with(p)
            {
                continue;
            }
            out.push(gc_registry::RefsListEntry {
                name: name.clone(),
                hash: Some(hash.clone()),
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    fn refs_set(
        &self,
        req: &gc_registry::RefsSetReq<'_>,
    ) -> Result<gc_registry::RefsSetResp, gc_registry::RegistryError> {
        // Server-side policy gating (minimal but real):
        // - policy artifact must exist and parse
        // - ref must match a policy class and not be frozen
        // - commit must exist and satisfy required obligations
        // - evidence artifacts referenced by commit must exist and parse as evidence
        // - commit pointers (base/patch/result) must exist
        let g = self.st.lock().expect("lock");

        let pol_bytes = g.store.get(req.policy).ok_or_else(|| {
            gc_registry::RegistryError::Protocol("refs/set: policy not found".to_string())
        })?;
        let pol_s = String::from_utf8(pol_bytes.clone()).map_err(|_| {
            gc_registry::RegistryError::Protocol("refs/set: bad policy utf8".to_string())
        })?;
        let pol_term = parse_term(&pol_s).map_err(|e| {
            gc_registry::RegistryError::Protocol(format!("refs/set: bad policy term: {e}"))
        })?;
        let pol = gc_vcs::Policy::from_term(&pol_term).map_err(|e| {
            gc_registry::RegistryError::Protocol(format!("refs/set: bad policy schema: {e}"))
        })?;

        if pol.is_frozen_ref(req.name) {
            return Err(gc_registry::RegistryError::Protocol(
                "refs/set: ref frozen".to_string(),
            ));
        }
        let class = pol.class_for_ref(req.name).ok_or_else(|| {
            gc_registry::RegistryError::Protocol("refs/set: no matching policy class".to_string())
        })?;

        let commit_bytes = g.store.get(req.hash).ok_or_else(|| {
            gc_registry::RegistryError::Protocol("refs/set: commit not found".to_string())
        })?;
        let commit_s = String::from_utf8(commit_bytes.clone()).map_err(|_| {
            gc_registry::RegistryError::Protocol("refs/set: bad commit utf8".to_string())
        })?;
        let commit_term = parse_term(&commit_s).map_err(|e| {
            gc_registry::RegistryError::Protocol(format!("refs/set: bad commit term: {e}"))
        })?;
        let commit = gc_vcs::Commit::from_term(&commit_term).map_err(|e| {
            gc_registry::RegistryError::Protocol(format!("refs/set: bad commit schema: {e}"))
        })?;

        if let Some(b) = commit.base.as_ref()
            && !g.store.contains_key(b)
        {
            return Err(gc_registry::RegistryError::Protocol(
                "refs/set: commit base missing".to_string(),
            ));
        }
        if !g.store.contains_key(&commit.patch) {
            return Err(gc_registry::RegistryError::Protocol(
                "refs/set: commit patch missing".to_string(),
            ));
        }
        if !g.store.contains_key(&commit.result) {
            return Err(gc_registry::RegistryError::Protocol(
                "refs/set: commit result snapshot missing".to_string(),
            ));
        }

        for req_ob in &class.required_obligations {
            if !commit.obligations.iter().any(|o| o == req_ob) {
                return Err(gc_registry::RegistryError::Protocol(
                    "refs/set: missing obligation".to_string(),
                ));
            }
        }
        if !class.required_obligations.is_empty() && commit.evidence.is_empty() {
            return Err(gc_registry::RegistryError::Protocol(
                "refs/set: missing evidence".to_string(),
            ));
        }
        let mut evidence_kinds: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        for ev_h in &commit.evidence {
            let ev_bytes = g.store.get(ev_h).ok_or_else(|| {
                gc_registry::RegistryError::Protocol("refs/set: evidence not found".to_string())
            })?;
            let ev_s = String::from_utf8(ev_bytes.clone()).map_err(|_| {
                gc_registry::RegistryError::Protocol("refs/set: evidence utf8".to_string())
            })?;
            let ev_t = parse_term(&ev_s).map_err(|e| {
                gc_registry::RegistryError::Protocol(format!("refs/set: bad evidence term: {e}"))
            })?;
            let ev = gc_vcs::Evidence::from_term(&ev_t).map_err(|e| {
                gc_registry::RegistryError::Protocol(format!("refs/set: bad evidence schema: {e}"))
            })?;
            evidence_kinds.insert(gc_vcs::PolicyClass::normalize_evidence_kind(&ev.kind));
        }
        let missing_kinds =
            class.missing_required_evidence_kinds(&commit.obligations, &evidence_kinds);
        if !missing_kinds.is_empty() {
            return Err(gc_registry::RegistryError::Protocol(
                "refs/set: missing evidence kinds".to_string(),
            ));
        }

        drop(g);

        let mut g = self.st.lock().expect("lock");
        let cur = g.refs.get(req.name).cloned();
        if let Some(exp) = req.expected_old
            && cur.as_deref() != Some(exp)
        {
            return Err(gc_registry::RegistryError::Http(
                "refs/set: status 409".to_string(),
            ));
        }
        g.refs.insert(req.name.to_string(), req.hash.to_string());

        Ok(gc_registry::RefsSetResp {
            ok: true,
            name: req.name.to_string(),
            hash: req.hash.to_string(),
        })
    }
}

fn mk_caps_for_sync(
    store_dir: &std::path::Path,
    refs_path: &std::path::Path,
    remote_allow: &str,
) -> CapsPolicy {
    mk_caps_for_sync_with_limits_and_auth(
        store_dir,
        refs_path,
        remote_allow,
        None,
        None,
        None,
        None,
    )
}

fn mk_caps_for_sync_with_auth_token(
    store_dir: &std::path::Path,
    refs_path: &std::path::Path,
    remote_allow: &str,
    auth_token: &str,
) -> CapsPolicy {
    mk_caps_for_sync_with_limits_and_auth(
        store_dir,
        refs_path,
        remote_allow,
        None,
        None,
        Some(auth_token),
        None,
    )
}

fn mk_caps_for_sync_with_basic_auth(
    store_dir: &std::path::Path,
    refs_path: &std::path::Path,
    remote_allow: &str,
    basic_username: &str,
    basic_password: &str,
) -> CapsPolicy {
    let s = format!(
        r#"
allow = ["core/sync::push", "core/sync::pull"]

[store]
dir = "{store_dir}"

[refs]
path = "{refs_path}"

[op."core/sync::push"]
remote_allow = ["{remote_allow}"]
transfer_workers = 4
basic_username = "{basic_username}"
basic_password = "{basic_password}"

[op."core/sync::pull"]
remote_allow = ["{remote_allow}"]
transfer_workers = 4
basic_username = "{basic_username}"
basic_password = "{basic_password}"
"#,
        store_dir = store_dir.display(),
        refs_path = refs_path.display(),
        remote_allow = remote_allow,
        basic_username = basic_username,
        basic_password = basic_password
    );
    CapsPolicy::from_toml_str(&s).expect("caps")
}

fn mk_caps_for_sync_with_mtls_files(
    store_dir: &std::path::Path,
    refs_path: &std::path::Path,
    remote_allow: &str,
    ca_pem_path: &std::path::Path,
    identity_pem_path: &std::path::Path,
) -> CapsPolicy {
    let s = format!(
        r#"
allow = ["core/sync::push", "core/sync::pull"]

[store]
dir = "{store_dir}"

[refs]
path = "{refs_path}"

[op."core/sync::push"]
remote_allow = ["{remote_allow}"]
transfer_workers = 4
mtls_ca_pem = "{ca_pem_path}"
mtls_identity_pem = "{identity_pem_path}"

[op."core/sync::pull"]
remote_allow = ["{remote_allow}"]
transfer_workers = 4
mtls_ca_pem = "{ca_pem_path}"
mtls_identity_pem = "{identity_pem_path}"
"#,
        store_dir = store_dir.display(),
        refs_path = refs_path.display(),
        remote_allow = remote_allow,
        ca_pem_path = ca_pem_path.display(),
        identity_pem_path = identity_pem_path.display(),
    );
    CapsPolicy::from_toml_str(&s).expect("caps")
}

fn mk_caps_for_sync_with_limits(
    store_dir: &std::path::Path,
    refs_path: &std::path::Path,
    remote_allow: &str,
    max_artifact_bytes: Option<usize>,
    max_batch_bytes: Option<usize>,
) -> CapsPolicy {
    mk_caps_for_sync_with_limits_and_auth(
        store_dir,
        refs_path,
        remote_allow,
        max_artifact_bytes,
        max_batch_bytes,
        None,
        None,
    )
}

fn mk_caps_for_sync_with_limits_and_auth(
    store_dir: &std::path::Path,
    refs_path: &std::path::Path,
    remote_allow: &str,
    max_artifact_bytes: Option<usize>,
    max_batch_bytes: Option<usize>,
    auth_token: Option<&str>,
    auth_token_env: Option<&str>,
) -> CapsPolicy {
    let pull_limits = {
        let mut parts: Vec<String> = Vec::new();
        if let Some(v) = max_artifact_bytes {
            parts.push(format!("max_artifact_bytes = {v}"));
        }
        if let Some(v) = max_batch_bytes {
            parts.push(format!("max_batch_bytes = {v}"));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("\n{}\n", parts.join("\n"))
        }
    };
    let auth_pull = match (auth_token, auth_token_env) {
        (Some(token), None) => format!("\nauth_token = \"{token}\""),
        (None, Some(env)) => format!("\nauth_token_env = \"{env}\""),
        _ => String::new(),
    };
    let auth_push = auth_pull.clone();
    let s = format!(
        r#"
allow = ["core/sync::push", "core/sync::pull"]

[store]
dir = "{store_dir}"

[refs]
path = "{refs_path}"

[op."core/sync::push"]
remote_allow = ["{remote_allow}"]
transfer_workers = 4
{auth_push}

[op."core/sync::pull"]
remote_allow = ["{remote_allow}"]
transfer_workers = 4
{auth_pull}
{pull_limits}
"#,
        store_dir = store_dir.display(),
        refs_path = refs_path.display(),
        remote_allow = remote_allow,
        pull_limits = pull_limits,
        auth_pull = auth_pull,
        auth_push = auth_push
    );
    CapsPolicy::from_toml_str(&s).expect("caps")
}

fn mk_caps_for_pkg_publish(
    store_dir: &std::path::Path,
    refs_path: &std::path::Path,
    remote_allow: &str,
) -> CapsPolicy {
    let s = format!(
        r#"
allow = ["core/pkg-low::publish"]

[store]
dir = "{store_dir}"

[refs]
path = "{refs_path}"

[op."core/pkg-low::publish"]
remote_allow = ["{remote_allow}"]
"#,
        store_dir = store_dir.display(),
        refs_path = refs_path.display(),
        remote_allow = remote_allow
    );
    CapsPolicy::from_toml_str(&s).expect("caps")
}

fn mk_prog(op: &str, payload: &Term) -> (Vec<Term>, [u8; 32]) {
    // (def prog (core/effect::perform 'op (quote payload) (fn (r) (core/effect::pure r)))) prog
    let op_t = Term::list(vec![Term::symbol("quote"), Term::symbol(op)]);
    let payload_t = Term::list(vec![Term::symbol("quote"), payload.clone()]);
    let k = Term::list(vec![
        Term::symbol("fn"),
        Term::list(vec![Term::symbol("r")]),
        Term::list(vec![Term::symbol("core/effect::pure"), Term::symbol("r")]),
    ]);
    let perform = Term::list(vec![
        Term::symbol("core/effect::perform"),
        op_t,
        payload_t,
        k,
    ]);
    let forms = vec![
        Term::list(vec![Term::symbol("def"), Term::symbol("prog"), perform]),
        Term::symbol("prog"),
    ];
    let h = gc_coreform::hash_module(&forms);
    (forms, h)
}

fn mk_policy_artifact() -> Term {
    parse_term(
        r#"
        {
          :type :vcs/policy
          :v 1
          :name "policy:test"
          :refs { :frozen-prefixes [] }
          :classes {
            :dev  { :patterns ["refs/**/heads/*"] :exclude ["refs/**/heads/main"] :required-obligations [] }
            :main { :patterns ["refs/**/heads/main"] :required-obligations [core/obligation::unit-tests] :require-signatures false }
            :tags { :patterns ["refs/**/tags/*"] :required-obligations [core/obligation::unit-tests] :require-signatures false }
          }
        }
        "#,
    )
    .expect("policy term")
}

fn mk_policy_artifact_with_obligation_evidence_kinds() -> Term {
    parse_term(
        r#"
        {
          :type :vcs/policy
          :v 1
          :name "policy:test-kinds"
          :refs { :frozen-prefixes [] }
          :classes {
            :main {
              :patterns ["refs/**/heads/main"]
              :required-obligations [core/obligation::unit-tests]
              :obligation-evidence-kinds { core/obligation::unit-tests [:effect-log] }
              :require-signatures false
            }
          }
        }
        "#,
    )
    .expect("policy term")
}

fn mk_evidence_with_data(data_hex: &str) -> Term {
    parse_term(&format!(
        r#"{{
          :type :vcs/evidence
          :v 1
          :kind :unit-tests
          :inputs []
          :outputs []
          :data "{data_hex}"
        }}"#
    ))
    .expect("evidence term")
}

fn mk_evidence_of_kind(kind: &str) -> Term {
    parse_term(&format!(
        r#"{{
          :type :vcs/evidence
          :v 1
          :kind {kind}
          :inputs []
          :outputs []
          :data nil
        }}"#
    ))
    .expect("evidence term")
}

fn mk_patch_with_value(value_hex: &str) -> Term {
    parse_term(&format!(
        r#"{{
          :type :vcs/patch
          :v 1
          :ops [
            {{ :op :replace :path [] :value "{value_hex}" }}
          ]
        }}"#
    ))
    .expect("patch term")
}

fn mk_snapshot(module_hex: &str, module_h: [u8; 32]) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":type")),
                Term::symbol(":vcs/snapshot"),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (TermOrdKey(Term::symbol(":kind")), Term::symbol(":package")),
            (
                TermOrdKey(Term::symbol(":pkg/name")),
                Term::Str("my-lib".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":pkg/version")),
                Term::Str("0.1.0".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":modules")),
                Term::Vector(vec![Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":path")),
                            Term::Str("m.gc".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":hash")),
                            Term::Str(module_hex.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":module-h")),
                            Term::Bytes(module_h.to_vec().into()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                )]),
            ),
            (
                TermOrdKey(Term::symbol(":obligations")),
                Term::Vector(vec![Term::symbol("core/obligation::unit-tests")]),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn mk_commit(result_hex: &str, patch_hex: &str, evidence_hex: &str) -> Term {
    parse_term(&format!(
        r#"{{
          :type :vcs/commit
          :v 1
          :parents []
          :target {{ :kind :package :name "my-lib" }}
          :base nil
          :patch "{patch_hex}"
          :result "{result_hex}"
          :obligations [core/obligation::unit-tests]
          :evidence ["{evidence_hex}"]
          :attestations []
          :message "sync test"
        }}"#
    ))
    .expect("commit term")
}

fn is_sealed_error(ctx: &EvalCtx, v: &Value, code: &str) -> bool {
    let Some(proto) = ctx.protocol else {
        return false;
    };
    let Value::Sealed { token, payload } = v else {
        return false;
    };
    if *token != proto.error {
        return false;
    }
    let Value::Data(Term::Map(m)) = payload.as_ref() else {
        return false;
    };
    matches!(
        m.get(&TermOrdKey(Term::symbol(":error/code"))),
        Some(Term::Str(s)) if s == code
    )
}

fn mk_remote(id: &str) -> (String, String) {
    let remote = format!("inproc://{id}/");
    let allow = gc_registry::normalize_remote_base(&remote)
        .expect("normalize")
        .as_str()
        .to_string();
    (remote, allow)
}

#[path = "sync_registry_cases_a.rs"]
mod sync_registry_cases_a;

#[path = "sync_registry_cases_b.rs"]
mod sync_registry_cases_b;
