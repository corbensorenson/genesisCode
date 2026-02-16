use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use fs2::FileExt;
use gc_coreform::{Term, TermOrdKey};
use reqwest::StatusCode;
use reqwest::Url;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub trait InProcRegistry: Send + Sync {
    fn ping(&self) -> Result<PingResp, RegistryError>;
    fn store_has(&self, hashes: &[String]) -> Result<BTreeMap<String, bool>, RegistryError>;
    fn store_get(&self, hash: &str) -> Result<Vec<u8>, RegistryError>;
    fn store_put(&self, hash: &str, bytes: &[u8]) -> Result<(), RegistryError>;
    fn refs_get(&self, name: &str) -> Result<Option<String>, RegistryError>;
    fn refs_list(&self, prefix: Option<&str>) -> Result<Vec<RefsListEntry>, RegistryError>;
    fn refs_set(&self, req: &RefsSetReq<'_>) -> Result<RefsSetResp, RegistryError>;
}

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("remote spec error: {0}")]
    RemoteSpec(String),

    #[error("http error: {0}")]
    Http(String),

    #[error("protocol error: {0}")]
    Protocol(String),
}

fn inproc_map() -> &'static Mutex<BTreeMap<String, Arc<dyn InProcRegistry>>> {
    static MAP: OnceLock<Mutex<BTreeMap<String, Arc<dyn InProcRegistry>>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub fn register_inproc(id: &str, reg: Arc<dyn InProcRegistry>) {
    let mut g = inproc_map().lock().expect("inproc registry lock");
    g.insert(id.to_string(), reg);
}

pub fn unregister_inproc(id: &str) {
    let mut g = inproc_map().lock().expect("inproc registry lock");
    g.remove(id);
}

#[derive(Debug, Clone)]
pub struct RegistryClient {
    base: Url,
    kind: RegistryKind,
}

#[derive(Debug, Clone)]
enum RegistryKind {
    Http { http: Client },
    InProc { id: String },
    File { root: PathBuf },
}

#[derive(Debug, Clone, Deserialize)]
pub struct PingResp {
    pub ok: bool,
    pub version: String,
    pub hash: String,
    pub max_chunk_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
struct StoreHasReq<'a> {
    hashes: &'a [String],
}

#[derive(Debug, Clone, Deserialize)]
struct StoreHasResp {
    present: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RefsGetResp {
    pub name: String,
    pub hash: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RefsListEntry {
    pub name: String,
    pub hash: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RefsListResp {
    refs: Vec<RefsListEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RefsSetReq<'a> {
    pub name: &'a str,
    pub hash: &'a str,
    pub policy: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_old: Option<&'a str>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RefsSetResp {
    pub ok: bool,
    pub name: String,
    pub hash: String,
}

impl RegistryClient {
    pub fn new(remote: &str, timeout: Option<Duration>) -> Result<Self, RegistryError> {
        let base = normalize_remote_base(remote)?;
        let kind = match base.scheme() {
            "https" | "http" => {
                let mut b = Client::builder();
                if let Some(t) = timeout {
                    b = b.timeout(t);
                }
                let http = b
                    .build()
                    .map_err(|e| RegistryError::Http(format!("build client: {e}")))?;
                RegistryKind::Http { http }
            }
            "inproc" => {
                let id = base.host_str().ok_or_else(|| {
                    RegistryError::RemoteSpec("inproc remote missing host".to_string())
                })?;
                RegistryKind::InProc { id: id.to_string() }
            }
            "file" => {
                let root = base.to_file_path().map_err(|_| {
                    RegistryError::RemoteSpec("file remote is not a valid path".to_string())
                })?;
                RegistryKind::File { root }
            }
            other => {
                return Err(RegistryError::RemoteSpec(format!(
                    "unsupported scheme {other}"
                )));
            }
        };
        Ok(Self { base, kind })
    }

    pub fn base_url(&self) -> &Url {
        &self.base
    }

    pub fn ping(&self) -> Result<PingResp, RegistryError> {
        if let RegistryKind::InProc { id } = &self.kind {
            let g = inproc_map().lock().expect("inproc registry lock");
            let reg = g.get(id).ok_or_else(|| {
                RegistryError::RemoteSpec(format!("inproc registry not registered: {id}"))
            })?;
            return reg.ping();
        }
        if let RegistryKind::File { .. } = &self.kind {
            return Ok(PingResp {
                ok: true,
                version: "0.1".to_string(),
                hash: "blake3-256".to_string(),
                max_chunk_bytes: None,
            });
        }
        let u = self
            .base
            .join("ping")
            .map_err(|e| RegistryError::RemoteSpec(format!("join ping: {e}")))?;
        let r = self
            .http()
            .get(u)
            .send()
            .map_err(|e| RegistryError::Http(format!("ping: {e}")))?;
        if !r.status().is_success() {
            return Err(RegistryError::Http(format!("ping: status {}", r.status())));
        }
        r.json::<PingResp>()
            .map_err(|e| RegistryError::Protocol(format!("ping json: {e}")))
    }

    pub fn store_has(&self, hashes: &[String]) -> Result<BTreeMap<String, bool>, RegistryError> {
        if let RegistryKind::InProc { id } = &self.kind {
            let g = inproc_map().lock().expect("inproc registry lock");
            let reg = g.get(id).ok_or_else(|| {
                RegistryError::RemoteSpec(format!("inproc registry not registered: {id}"))
            })?;
            return reg.store_has(hashes);
        }
        if let RegistryKind::File { root } = &self.kind {
            file_ensure_dirs(root)?;
            let mut out = BTreeMap::new();
            for h in hashes {
                out.insert(h.clone(), file_store_path(root, h).exists());
            }
            return Ok(out);
        }
        let u = self
            .base
            .join("store/has")
            .map_err(|e| RegistryError::RemoteSpec(format!("join store/has: {e}")))?;
        let r = self
            .http()
            .post(u)
            .json(&StoreHasReq { hashes })
            .send()
            .map_err(|e| RegistryError::Http(format!("store/has: {e}")))?;
        if !r.status().is_success() {
            return Err(RegistryError::Http(format!(
                "store/has: status {}",
                r.status()
            )));
        }
        let resp = r
            .json::<StoreHasResp>()
            .map_err(|e| RegistryError::Protocol(format!("store/has json: {e}")))?;
        Ok(resp.present)
    }

    pub fn store_get(&self, hash: &str) -> Result<Vec<u8>, RegistryError> {
        if let RegistryKind::InProc { id } = &self.kind {
            let g = inproc_map().lock().expect("inproc registry lock");
            let reg = g.get(id).ok_or_else(|| {
                RegistryError::RemoteSpec(format!("inproc registry not registered: {id}"))
            })?;
            return reg.store_get(hash);
        }
        if let RegistryKind::File { root } = &self.kind {
            file_ensure_dirs(root)?;
            let p = file_store_path(root, hash);
            if !p.exists() {
                return Err(RegistryError::Http("store/get: status 404".to_string()));
            }
            let bytes = std::fs::read(&p).map_err(|e| RegistryError::Http(format!("{e}")))?;
            let got = blake3::hash(&bytes).to_hex().to_string();
            if got != hash {
                return Err(RegistryError::Protocol("store/get: hash mismatch".to_string()));
            }
            return Ok(bytes);
        }
        let u = self
            .base
            .join(&format!("store/get/{hash}"))
            .map_err(|e| RegistryError::RemoteSpec(format!("join store/get: {e}")))?;
        let r = self
            .http()
            .get(u)
            .send()
            .map_err(|e| RegistryError::Http(format!("store/get: {e}")))?;
        if !r.status().is_success() {
            return Err(RegistryError::Http(format!(
                "store/get: status {}",
                r.status()
            )));
        }
        r.bytes()
            .map(|b| b.to_vec())
            .map_err(|e| RegistryError::Http(format!("store/get bytes: {e}")))
    }

    pub fn store_get_opt(&self, hash: &str) -> Result<Option<Vec<u8>>, RegistryError> {
        if let RegistryKind::InProc { id } = &self.kind {
            let g = inproc_map().lock().expect("inproc registry lock");
            let reg = g.get(id).ok_or_else(|| {
                RegistryError::RemoteSpec(format!("inproc registry not registered: {id}"))
            })?;
            return match reg.store_get(hash) {
                Ok(b) => Ok(Some(b)),
                Err(RegistryError::Http(s)) if s.contains("status 404") => Ok(None),
                Err(e) => Err(e),
            };
        }
        if let RegistryKind::File { root } = &self.kind {
            file_ensure_dirs(root)?;
            let p = file_store_path(root, hash);
            if !p.exists() {
                return Ok(None);
            }
            let bytes = std::fs::read(&p).map_err(|e| RegistryError::Http(format!("{e}")))?;
            let got = blake3::hash(&bytes).to_hex().to_string();
            if got != hash {
                return Err(RegistryError::Protocol("store/get: hash mismatch".to_string()));
            }
            return Ok(Some(bytes));
        }
        let u = self
            .base
            .join(&format!("store/get/{hash}"))
            .map_err(|e| RegistryError::RemoteSpec(format!("join store/get: {e}")))?;
        let r = self
            .http()
            .get(u)
            .send()
            .map_err(|e| RegistryError::Http(format!("store/get: {e}")))?;
        if r.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !r.status().is_success() {
            return Err(RegistryError::Http(format!(
                "store/get: status {}",
                r.status()
            )));
        }
        r.bytes()
            .map(|b| Some(b.to_vec()))
            .map_err(|e| RegistryError::Http(format!("store/get bytes: {e}")))
    }

    pub fn store_put(&self, hash: &str, bytes: &[u8]) -> Result<(), RegistryError> {
        if let RegistryKind::InProc { id } = &self.kind {
            let g = inproc_map().lock().expect("inproc registry lock");
            let reg = g.get(id).ok_or_else(|| {
                RegistryError::RemoteSpec(format!("inproc registry not registered: {id}"))
            })?;
            return reg.store_put(hash, bytes);
        }
        if let RegistryKind::File { root } = &self.kind {
            file_ensure_dirs(root)?;
            let got = blake3::hash(bytes).to_hex().to_string();
            if got != hash {
                return Err(RegistryError::Protocol("store/put: hash mismatch".to_string()));
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
            file_atomic_write(&p, bytes)?;
            return Ok(());
        }
        let u = self
            .base
            .join(&format!("store/put/{hash}"))
            .map_err(|e| RegistryError::RemoteSpec(format!("join store/put: {e}")))?;
        let r = self
            .http()
            .put(u)
            .body(bytes.to_vec())
            .send()
            .map_err(|e| RegistryError::Http(format!("store/put: {e}")))?;
        if !r.status().is_success() {
            return Err(RegistryError::Http(format!(
                "store/put: status {}",
                r.status()
            )));
        }
        Ok(())
    }

    pub fn refs_get(&self, name: &str) -> Result<Option<String>, RegistryError> {
        if let RegistryKind::InProc { id } = &self.kind {
            let g = inproc_map().lock().expect("inproc registry lock");
            let reg = g.get(id).ok_or_else(|| {
                RegistryError::RemoteSpec(format!("inproc registry not registered: {id}"))
            })?;
            return reg.refs_get(name);
        }
        if let RegistryKind::File { root } = &self.kind {
            file_ensure_dirs(root)?;
            let _lk = file_refs_lock(root)?;
            let refs = file_load_refs_locked(root)?;
            return Ok(refs.get(name).cloned());
        }
        let mut u = self
            .base
            .join("refs/get")
            .map_err(|e| RegistryError::RemoteSpec(format!("join refs/get: {e}")))?;
        u.query_pairs_mut().append_pair("name", name);
        let r = self
            .http()
            .get(u)
            .send()
            .map_err(|e| RegistryError::Http(format!("refs/get: {e}")))?;
        if !r.status().is_success() {
            return Err(RegistryError::Http(format!(
                "refs/get: status {}",
                r.status()
            )));
        }
        let resp = r
            .json::<RefsGetResp>()
            .map_err(|e| RegistryError::Protocol(format!("refs/get json: {e}")))?;
        Ok(resp.hash)
    }

    pub fn refs_list(&self, prefix: Option<&str>) -> Result<Vec<RefsListEntry>, RegistryError> {
        if let RegistryKind::InProc { id } = &self.kind {
            let g = inproc_map().lock().expect("inproc registry lock");
            let reg = g.get(id).ok_or_else(|| {
                RegistryError::RemoteSpec(format!("inproc registry not registered: {id}"))
            })?;
            return reg.refs_list(prefix);
        }
        if let RegistryKind::File { root } = &self.kind {
            file_ensure_dirs(root)?;
            let _lk = file_refs_lock(root)?;
            let refs = file_load_refs_locked(root)?;
            let mut out = Vec::new();
            for (name, hash) in refs {
                if let Some(p) = prefix
                    && !name.starts_with(p)
                {
                    continue;
                }
                out.push(RefsListEntry {
                    name,
                    hash: Some(hash),
                });
            }
            out.sort_by(|a, b| a.name.cmp(&b.name));
            return Ok(out);
        }
        let mut u = self
            .base
            .join("refs/list")
            .map_err(|e| RegistryError::RemoteSpec(format!("join refs/list: {e}")))?;
        if let Some(p) = prefix {
            u.query_pairs_mut().append_pair("prefix", p);
        }
        let r = self
            .http()
            .get(u)
            .send()
            .map_err(|e| RegistryError::Http(format!("refs/list: {e}")))?;
        if !r.status().is_success() {
            return Err(RegistryError::Http(format!(
                "refs/list: status {}",
                r.status()
            )));
        }
        let resp = r
            .json::<RefsListResp>()
            .map_err(|e| RegistryError::Protocol(format!("refs/list json: {e}")))?;
        Ok(resp.refs)
    }

    pub fn refs_set(&self, req: &RefsSetReq<'_>) -> Result<RefsSetResp, RegistryError> {
        if let RegistryKind::InProc { id } = &self.kind {
            let g = inproc_map().lock().expect("inproc registry lock");
            let reg = g.get(id).ok_or_else(|| {
                RegistryError::RemoteSpec(format!("inproc registry not registered: {id}"))
            })?;
            return reg.refs_set(req);
        }
        if let RegistryKind::File { root } = &self.kind {
            file_ensure_dirs(root)?;
            let mut lk = file_refs_lock(root)?;
            let mut refs = file_load_refs_locked(root)?;

            // Policy-gated before ref mutation.
            file_gate_refs_set(root, req.name, req.hash, req.policy)?;

            let cur = refs.get(req.name).cloned();
            if let Some(exp) = req.expected_old
                && cur.as_deref() != Some(exp)
            {
                return Err(RegistryError::Http("refs/set: status 409".to_string()));
            }
            refs.insert(req.name.to_string(), req.hash.to_string());
            file_write_refs_locked(root, &refs, &mut lk)?;
            return Ok(RefsSetResp {
                ok: true,
                name: req.name.to_string(),
                hash: req.hash.to_string(),
            });
        }
        let u = self
            .base
            .join("refs/set")
            .map_err(|e| RegistryError::RemoteSpec(format!("join refs/set: {e}")))?;
        let r = self
            .http()
            .post(u)
            .json(req)
            .send()
            .map_err(|e| RegistryError::Http(format!("refs/set: {e}")))?;
        if !r.status().is_success() {
            return Err(RegistryError::Http(format!(
                "refs/set: status {}",
                r.status()
            )));
        }
        r.json::<RefsSetResp>()
            .map_err(|e| RegistryError::Protocol(format!("refs/set json: {e}")))
    }

    fn http(&self) -> &Client {
        match &self.kind {
            RegistryKind::Http { http } => http,
            RegistryKind::InProc { .. } | RegistryKind::File { .. } => unreachable!(
                "http client requested for non-http registry"
            ),
        }
    }
}

pub fn normalize_remote_base(remote: &str) -> Result<Url, RegistryError> {
    let t = remote.trim();
    if t.is_empty() {
        return Err(RegistryError::RemoteSpec("remote is empty".to_string()));
    }
    let mut u = if t.starts_with("gen://") {
        let rest = t.strip_prefix("gen://").unwrap_or("");
        Url::parse(&format!("https://{rest}"))
            .map_err(|e| RegistryError::RemoteSpec(format!("bad gen:// url: {e}")))?
    } else {
        Url::parse(t).map_err(|e| RegistryError::RemoteSpec(format!("bad url: {e}")))?
    };
    if u.scheme() != "https" && u.scheme() != "http" && u.scheme() != "inproc" && u.scheme() != "file" {
        return Err(RegistryError::RemoteSpec(format!(
            "unsupported scheme {}",
            u.scheme()
        )));
    }

    // Normalize to .../v1/ base.
    let path = u.path().to_string();
    let base_path = if path.ends_with("/v1/") {
        path
    } else if path.ends_with("/v1") {
        format!("{path}/")
    } else if path.ends_with('/') || path.is_empty() {
        format!("{path}v1/")
    } else {
        format!("{path}/v1/")
    };
    u.set_path(&base_path);
    u.set_query(None);
    Ok(u)
}

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
        std::fs::create_dir_all(parent)
            .map_err(|e| RegistryError::Http(format!("mkdir: {e}")))?;
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
        std::fs::create_dir_all(parent)
            .map_err(|e| RegistryError::Http(format!("mkdir: {e}")))?;
    }
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&p)
        .map_err(|e| RegistryError::Http(format!("open refs lock: {e}")))?;
    f.lock_exclusive()
        .map_err(|e| RegistryError::Http(format!("lock refs: {e}")))?;
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
        return Err(RegistryError::Protocol("refs db: wrong or missing :v".to_string()));
    }
    let kind = m.get(&TermOrdKey(Term::symbol(":kind")));
    if !matches!(kind, Some(Term::Str(s)) if s == "genesis/refs-db-v0.1") {
        return Err(RegistryError::Protocol(
            "refs db: wrong or missing :kind".to_string(),
        ));
    }
    let Some(Term::Map(refs)) = m.get(&TermOrdKey(Term::symbol(":refs"))) else {
        return Err(RegistryError::Protocol("refs db: missing :refs map".to_string()));
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

fn file_gate_refs_set(root: &Path, name: &str, commit_h: &str, policy_h: &str) -> Result<(), RegistryError> {
    // Resolve policy term from remote store.
    let pol_bytes = std::fs::read(file_store_path(root, policy_h))
        .map_err(|_| RegistryError::Http("refs/set: status 403".to_string()))?;
    if blake3::hash(&pol_bytes).to_hex().to_string() != policy_h {
        return Err(RegistryError::Protocol("refs/set: policy corruption".to_string()));
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
    let class = pol.class_for_ref(name).ok_or_else(|| RegistryError::Http("refs/set: status 403".to_string()))?;

    let commit_bytes = std::fs::read(file_store_path(root, commit_h))
        .map_err(|_| RegistryError::Http("refs/set: status 403".to_string()))?;
    if blake3::hash(&commit_bytes).to_hex().to_string() != commit_h {
        return Err(RegistryError::Protocol("refs/set: commit corruption".to_string()));
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
    if !file_store_path(root, &commit.patch).exists() || !file_store_path(root, &commit.result).exists() {
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
    for ev_h in &commit.evidence {
        let ev_bytes = std::fs::read(file_store_path(root, ev_h))
            .map_err(|_| RegistryError::Http("refs/set: status 403".to_string()))?;
        if blake3::hash(&ev_bytes).to_hex().to_string() != *ev_h {
            return Err(RegistryError::Protocol("refs/set: evidence corruption".to_string()));
        }
        let ev_s = String::from_utf8(ev_bytes)
            .map_err(|_| RegistryError::Protocol("refs/set: evidence not utf8".to_string()))?;
        let ev_t = gc_coreform::parse_term(&ev_s)
            .map_err(|e| RegistryError::Protocol(format!("refs/set: bad evidence term: {e}")))?;
        gc_vcs::Evidence::from_term(&ev_t)
            .map_err(|e| RegistryError::Protocol(format!("refs/set: bad evidence schema: {e}")))?;
    }

    if class.require_signatures {
        let signing_h = gc_vcs::commit_signing_hash(&commit_term)
            .map_err(|e| RegistryError::Protocol(format!("refs/set: bad commit signing hash: {e}")))?;
        let mut valid: u64 = 0;
        let mut seen_pks: std::collections::BTreeSet<Vec<u8>> = std::collections::BTreeSet::new();
        for at_h in &commit.attestations {
            let at_bytes = std::fs::read(file_store_path(root, at_h))
                .map_err(|_| RegistryError::Http("refs/set: status 403".to_string()))?;
            if blake3::hash(&at_bytes).to_hex().to_string() != *at_h {
                return Err(RegistryError::Protocol("refs/set: attestation corruption".to_string()));
            }
            let at_s = String::from_utf8(at_bytes)
                .map_err(|_| RegistryError::Protocol("refs/set: attestation not utf8".to_string()))?;
            let at_t = gc_coreform::parse_term(&at_s)
                .map_err(|e| RegistryError::Protocol(format!("refs/set: bad attestation term: {e}")))?;
            let att = gc_vcs::Attestation::from_term(&at_t)
                .map_err(|e| RegistryError::Protocol(format!("refs/set: bad attestation schema: {e}")))?;
            if !seen_pks.insert(att.pk.to_vec()) {
                continue;
            }
            if gc_vcs::verify_commit_attestation(&att, &signing_h, &class.allowed_public_keys).is_ok() {
                valid = valid.saturating_add(1);
            }
        }
        if valid < class.min_signatures {
            return Err(RegistryError::Http("refs/set: status 403".to_string()));
        }
    }

    Ok(())
}
