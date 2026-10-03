use super::limits::{HttpRegistryServerLimits, limit, reserve};
use super::uploads::UploadState;
use crate::{
    RefsSetReq, RegistryClient, RegistryError, StoreUploadChunkResp, StoreUploadFinishResp,
    StoreUploadStartResp, StoreUploadStatusResp,
};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;

#[derive(Debug, Clone, Deserialize)]
struct StoreHasReqOwned {
    #[serde(deserialize_with = "bounded_hashes")]
    hashes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct StoreHasRespOwned {
    present: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, Deserialize)]
struct StoreUploadStartReqOwned {
    hash: String,
    size_bytes: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct StoreUploadFinishReqOwned {
    upload_id: String,
}

#[derive(Debug, Clone, Serialize)]
struct RefsGetRespOwned {
    name: String,
    hash: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct RefsListRespOwned {
    refs: Vec<crate::RefsListEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct RefsSetReqOwned {
    name: String,
    hash: String,
    policy: String,
    expected_old: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Debug, Clone, Serialize)]
struct ErrorBody {
    code: String,
    message: String,
}

pub(super) fn dispatch(
    method: &hyper::Method,
    parsed: &ParsedReqUrl,
    body: Vec<u8>,
    client: &RegistryClient,
    uploads: &mut UploadState,
    max_chunk_bytes: u64,
    limits: &HttpRegistryServerLimits,
) -> Result<(u16, &'static str, Vec<u8>), RegistryError> {
    let plan: Result<(u16, &'static str, Vec<u8>), RegistryError> = (|| {
        if parsed.path == "v1/ping" && *method == hyper::Method::GET {
            let body = encode_json(
                limits.max_json_bytes,
                &serde_json::json!({
                    "ok": true,
                    "version": "0.1",
                    "hash": "blake3-256",
                    "max_chunk_bytes": max_chunk_bytes
                }),
            )?;
            return Ok((200, "application/json", body));
        }

        match (method.as_str(), parsed.path.as_str()) {
            ("POST", "v1/store/has") => {
                let in_req: StoreHasReqOwned = read_json(&body)?;
                let present = client.store_has(&in_req.hashes)?;
                let body = encode_json(limits.max_json_bytes, &StoreHasRespOwned { present })?;
                Ok((200, "application/json", body))
            }
            ("GET", p) if p.starts_with("v1/store/get/") => {
                let hash = p.trim_start_matches("v1/store/get/");
                let bytes = client.store_get_bounded(hash, Some(limits.max_object_bytes))?;
                Ok((200, "application/octet-stream", bytes))
            }
            ("PUT", p) if p.starts_with("v1/store/put/") => {
                let hash = p.trim_start_matches("v1/store/put/");
                let bytes = body;
                client.store_put(hash, &bytes)?;
                Ok((200, "application/json", b"{}".to_vec()))
            }
            ("POST", "v1/store/upload/start") => {
                let in_req: StoreUploadStartReqOwned = read_json(&body)?;
                let upload_id =
                    uploads.start(in_req.hash, in_req.size_bytes, max_chunk_bytes, limits)?;
                Ok((
                    200,
                    "application/json",
                    encode_json(
                        limits.max_json_bytes,
                        &StoreUploadStartResp {
                            upload_id,
                            chunk_bytes: max_chunk_bytes,
                        },
                    )?,
                ))
            }
            ("PUT", p) if p.starts_with("v1/store/upload/chunk/") => {
                let (id, index) = chunk_path(p)?;
                let received = uploads.insert(id, index, body, limits)?;
                Ok((
                    200,
                    "application/json",
                    encode_json(
                        limits.max_json_bytes,
                        &StoreUploadChunkResp {
                            ok: true,
                            received: received as u64,
                        },
                    )?,
                ))
            }
            ("POST", "v1/store/upload/finish") => {
                let in_req: StoreUploadFinishReqOwned = read_json(&body)?;
                let (hash, payload) = uploads.finish(&in_req.upload_id, limits)?;
                client.store_put(&hash, &payload)?;
                Ok((
                    200,
                    "application/json",
                    encode_json(limits.max_json_bytes, &StoreUploadFinishResp { ok: true })?,
                ))
            }
            ("GET", p) if p.starts_with("v1/store/upload/status/") => {
                let received_chunks =
                    uploads.status(p.trim_start_matches("v1/store/upload/status/"), limits)?;
                Ok((
                    200,
                    "application/json",
                    encode_json(
                        limits.max_json_bytes,
                        &StoreUploadStatusResp { received_chunks },
                    )?,
                ))
            }
            ("GET", "v1/refs/get") => {
                let name = parsed.query.get("name").cloned().ok_or_else(|| {
                    RegistryError::Protocol("refs/get: missing query parameter `name`".to_string())
                })?;
                let hash = client.refs_get(&name)?;
                let body = encode_json(limits.max_json_bytes, &RefsGetRespOwned { name, hash })?;
                Ok((200, "application/json", body))
            }
            ("GET", "v1/refs/list") => {
                let prefix = parsed.query.get("prefix").map(String::as_str);
                let refs = client.refs_list(prefix)?;
                let body = encode_json(limits.max_json_bytes, &RefsListRespOwned { refs })?;
                Ok((200, "application/json", body))
            }
            ("POST", "v1/refs/set") => {
                let in_req: RefsSetReqOwned = read_json(&body)?;
                let out = client.refs_set(&RefsSetReq {
                    name: &in_req.name,
                    hash: &in_req.hash,
                    policy: &in_req.policy,
                    expected_old: in_req.expected_old.as_deref(),
                })?;
                let body = encode_json(limits.max_json_bytes, &out)?;
                Ok((200, "application/json", body))
            }
            _ => Err(RegistryError::Http("route: status 404".to_string())),
        }
    })();

    plan
}

fn read_json<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, RegistryError> {
    serde_json::from_slice(body).map_err(|e| {
        if e.to_string().starts_with("resource-limit:") {
            limit("JSON collection budget exceeded")
        } else {
            RegistryError::Protocol(format!("json decode: {e}"))
        }
    })
}

fn bounded_hashes<'de, D: serde::Deserializer<'de>>(decoder: D) -> Result<Vec<String>, D::Error> {
    struct Hashes;
    impl<'de> serde::de::Visitor<'de> for Hashes {
        type Value = Vec<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("at most 1024 object hashes")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut input: A,
        ) -> Result<Self::Value, A::Error> {
            let mut hashes = Vec::new();
            while hashes.len() < 1024 {
                let Some(hash) = input.next_element::<String>()? else {
                    return Ok(hashes);
                };
                crate::validate_store_hash(&hash).map_err(serde::de::Error::custom)?;
                hashes.try_reserve(1).map_err(|_| {
                    serde::de::Error::custom("resource-limit: hash array allocation failed")
                })?;
                hashes.push(hash);
            }
            if input.next_element::<serde::de::IgnoredAny>()?.is_some() {
                return Err(serde::de::Error::custom(
                    "resource-limit: hash array count exceeded",
                ));
            }
            Ok(hashes)
        }
    }
    decoder.deserialize_seq(Hashes)
}

struct JsonWriter {
    bytes: Vec<u8>,
    bound: usize,
}
impl Write for JsonWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.bound - self.bytes.len() {
            return Err(std::io::Error::other(
                "resource-limit: JSON response bytes exceeded",
            ));
        }
        reserve(&mut self.bytes, bytes.len(), self.bound).map_err(std::io::Error::other)?;
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn encode_json<T: Serialize>(bound: usize, value: &T) -> Result<Vec<u8>, RegistryError> {
    let mut output = JsonWriter {
        bytes: Vec::new(),
        bound,
    };
    serde_json::to_writer(&mut output, value)
        .map_err(|_| limit("JSON response serialization failed"))?;
    Ok(output.bytes)
}

pub(super) fn error_response(error: RegistryError, bound: usize) -> (u16, &'static str, Vec<u8>) {
    let (status, code, message) = registry_error_http(error);
    let envelope = ErrorEnvelope {
        error: ErrorBody { code, message },
    };
    match encode_json(bound, &envelope) {
        Ok(body) => (status, "application/json", body),
        Err(_) => (413, "application/json", b"{}".to_vec()),
    }
}

pub(super) fn chunk_path(path: &str) -> Result<(&str, u64), RegistryError> {
    let suffix = path.trim_start_matches("v1/store/upload/chunk/");
    let (id, raw_index) = suffix
        .split_once('/')
        .ok_or_else(|| RegistryError::Protocol("store/upload/chunk: missing index".to_string()))?;
    let index = raw_index
        .parse()
        .map_err(|_| RegistryError::Protocol("store/upload/chunk: invalid index".to_string()))?;
    Ok((id, index))
}

pub(super) fn body_limit(
    method: &hyper::Method,
    path: &str,
    uploads: &mut UploadState,
    chunk: usize,
    limits: &HttpRegistryServerLimits,
) -> Result<usize, RegistryError> {
    if *method == hyper::Method::GET {
        return Ok(0);
    }
    if *method == hyper::Method::PUT && path.starts_with("v1/store/upload/chunk/") {
        let (id, index) = chunk_path(path)?;
        return uploads.body_limit(id, index, chunk, limits);
    }
    if *method == hyper::Method::PUT && path.starts_with("v1/store/put/") {
        crate::validate_store_hash(path.trim_start_matches("v1/store/put/"))?;
        return Ok(limits.max_object_bytes);
    }
    Ok(limits.max_json_bytes)
}

pub(super) fn registry_error_http(err: RegistryError) -> (u16, String, String) {
    match err {
        RegistryError::Auth(msg) => (401, "unauthorized".to_string(), msg),
        RegistryError::RemoteSpec(msg) => (400, "bad_request".to_string(), msg),
        RegistryError::Protocol(msg) if msg.starts_with("resource-limit:") => {
            (413, "payload_too_large".to_string(), msg)
        }
        RegistryError::Protocol(msg) => (400, "protocol".to_string(), msg),
        error @ RegistryError::HashMismatch { .. } => {
            (400, "protocol".to_string(), error.to_string())
        }
        RegistryError::Http(msg) => {
            let status = parse_status_code_hint(&msg).unwrap_or(500);
            let code = match status {
                400 => "bad_request",
                401 => "unauthorized",
                403 => "forbidden",
                404 => "not_found",
                409 => "conflict",
                413 => "payload_too_large",
                _ => "internal",
            };
            (status, code.to_string(), msg)
        }
    }
}

fn parse_status_code_hint(msg: &str) -> Option<u16> {
    let needle = "status ";
    let idx = msg.find(needle)?;
    let raw = msg[idx + needle.len()..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>();
    raw.parse::<u16>().ok()
}

#[derive(Debug, Clone)]
pub(super) struct ParsedReqUrl {
    pub(super) path: String,
    query: BTreeMap<String, String>,
}

pub(super) fn parse_req_url(raw: &str) -> Result<ParsedReqUrl, RegistryError> {
    let u = Url::parse(&format!("http://localhost{raw}"))
        .map_err(|e| RegistryError::Protocol(format!("bad request url `{raw}`: {e}")))?;
    let mut query = BTreeMap::new();
    for (k, v) in u.query_pairs() {
        query.insert(k.to_string(), v.to_string());
    }
    Ok(ParsedReqUrl {
        path: u.path().trim_start_matches('/').to_string(),
        query,
    })
}
