use std::collections::BTreeMap;
use std::time::Duration;

use reqwest::Url;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("remote spec error: {0}")]
    RemoteSpec(String),

    #[error("http error: {0}")]
    Http(String),

    #[error("protocol error: {0}")]
    Protocol(String),
}

#[derive(Debug, Clone)]
pub struct RegistryClient {
    base: Url,
    http: Client,
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
        let mut b = Client::builder();
        if let Some(t) = timeout {
            b = b.timeout(t);
        }
        let http = b
            .build()
            .map_err(|e| RegistryError::Http(format!("build client: {e}")))?;
        Ok(Self { base, http })
    }

    pub fn base_url(&self) -> &Url {
        &self.base
    }

    pub fn ping(&self) -> Result<PingResp, RegistryError> {
        let u = self
            .base
            .join("ping")
            .map_err(|e| RegistryError::RemoteSpec(format!("join ping: {e}")))?;
        let r = self
            .http
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
        let u = self
            .base
            .join("store/has")
            .map_err(|e| RegistryError::RemoteSpec(format!("join store/has: {e}")))?;
        let r = self
            .http
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
        let u = self
            .base
            .join(&format!("store/get/{hash}"))
            .map_err(|e| RegistryError::RemoteSpec(format!("join store/get: {e}")))?;
        let r = self
            .http
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

    pub fn store_put(&self, hash: &str, bytes: &[u8]) -> Result<(), RegistryError> {
        let u = self
            .base
            .join(&format!("store/put/{hash}"))
            .map_err(|e| RegistryError::RemoteSpec(format!("join store/put: {e}")))?;
        let r = self
            .http
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
        let mut u = self
            .base
            .join("refs/get")
            .map_err(|e| RegistryError::RemoteSpec(format!("join refs/get: {e}")))?;
        u.query_pairs_mut().append_pair("name", name);
        let r = self
            .http
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
        let mut u = self
            .base
            .join("refs/list")
            .map_err(|e| RegistryError::RemoteSpec(format!("join refs/list: {e}")))?;
        if let Some(p) = prefix {
            u.query_pairs_mut().append_pair("prefix", p);
        }
        let r = self
            .http
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
        let u = self
            .base
            .join("refs/set")
            .map_err(|e| RegistryError::RemoteSpec(format!("join refs/set: {e}")))?;
        let r = self
            .http
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
    if u.scheme() != "https" && u.scheme() != "http" {
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
