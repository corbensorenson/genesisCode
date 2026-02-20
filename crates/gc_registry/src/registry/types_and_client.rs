pub const WASI_HTTP_BRIDGE_ROOT_ENV: &str = "GENESIS_WASI_HTTP_BRIDGE_ROOT";

pub trait InProcRegistry: Send + Sync {
    fn authorize(&self, _auth: &RegistryAuth) -> Result<(), RegistryError> {
        Ok(())
    }
    fn ping(&self) -> Result<PingResp, RegistryError>;
    fn store_has(&self, hashes: &[String]) -> Result<BTreeMap<String, bool>, RegistryError>;
    fn store_get(&self, hash: &str) -> Result<Vec<u8>, RegistryError>;
    fn store_put(&self, hash: &str, bytes: &[u8]) -> Result<(), RegistryError>;
    fn store_upload_start(
        &self,
        _hash: &str,
        _size_bytes: u64,
    ) -> Result<StoreUploadStartResp, RegistryError> {
        Err(RegistryError::Protocol(
            "store/upload/start: not supported".to_string(),
        ))
    }
    fn store_upload_chunk(
        &self,
        _upload_id: &str,
        _index: u64,
        _bytes: &[u8],
    ) -> Result<StoreUploadChunkResp, RegistryError> {
        Err(RegistryError::Protocol(
            "store/upload/chunk: not supported".to_string(),
        ))
    }
    fn store_upload_finish(
        &self,
        _upload_id: &str,
    ) -> Result<StoreUploadFinishResp, RegistryError> {
        Err(RegistryError::Protocol(
            "store/upload/finish: not supported".to_string(),
        ))
    }
    fn store_upload_status(
        &self,
        _upload_id: &str,
    ) -> Result<StoreUploadStatusResp, RegistryError> {
        Err(RegistryError::Protocol(
            "store/upload/status: not supported".to_string(),
        ))
    }
    fn refs_get(&self, name: &str) -> Result<Option<String>, RegistryError>;
    fn refs_list(&self, prefix: Option<&str>) -> Result<Vec<RefsListEntry>, RegistryError>;
    fn refs_set(&self, req: &RefsSetReq<'_>) -> Result<RefsSetResp, RegistryError>;
}

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("remote spec error: {0}")]
    RemoteSpec(String),

    #[error("auth error: {0}")]
    Auth(String),

    #[error("http error: {0}")]
    Http(String),

    #[error("protocol error: {0}")]
    Protocol(String),
}

type InProcRegistryMap = BTreeMap<String, Arc<dyn InProcRegistry>>;
type InProcRegistryMapGuard = std::sync::MutexGuard<'static, InProcRegistryMap>;

fn inproc_map() -> &'static Mutex<InProcRegistryMap> {
    static MAP: OnceLock<Mutex<InProcRegistryMap>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn lock_inproc_map() -> Result<InProcRegistryMapGuard, RegistryError> {
    inproc_map()
        .lock()
        .map_err(|_| RegistryError::Protocol("inproc registry lock poisoned".to_string()))
}

pub fn register_inproc(id: &str, reg: Arc<dyn InProcRegistry>) -> Result<(), RegistryError> {
    let mut g = lock_inproc_map()?;
    g.insert(id.to_string(), reg);
    Ok(())
}

pub fn unregister_inproc(id: &str) -> Result<(), RegistryError> {
    let mut g = lock_inproc_map()?;
    g.remove(id);
    Ok(())
}

#[derive(Debug, Clone)]
pub struct RegistryClient {
    base: Url,
    kind: RegistryKind,
    auth: RegistryAuth,
}

#[derive(Debug, Clone)]
enum RegistryKind {
    #[cfg(not(target_os = "wasi"))]
    Http {
        http: Client,
    },
    #[cfg(target_os = "wasi")]
    Http,
    InProc {
        id: String,
    },
    File {
        root: PathBuf,
    },
}

#[derive(Clone, Default)]
pub struct RegistryAuth {
    pub bearer_token: Option<String>,
    pub basic_username: Option<String>,
    pub basic_password: Option<String>,
    pub mtls_ca_pem: Option<Vec<u8>>,
    pub mtls_identity_pem: Option<Vec<u8>>,
}

impl std::fmt::Debug for RegistryAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistryAuth")
            .field(
                "bearer_token",
                &self.bearer_token.as_ref().map(|_| "<redacted>"),
            )
            .field(
                "basic_username",
                &self.basic_username.as_ref().map(|_| "<redacted>"),
            )
            .field(
                "basic_password",
                &self.basic_password.as_ref().map(|_| "<redacted>"),
            )
            .field("mtls_ca_pem", &self.mtls_ca_pem.as_ref().map(|b| b.len()))
            .field(
                "mtls_identity_pem",
                &self.mtls_identity_pem.as_ref().map(|b| b.len()),
            )
            .finish()
    }
}

impl RegistryAuth {
    fn has_any(&self) -> bool {
        self.bearer_token.is_some()
            || self.basic_username.is_some()
            || self.basic_password.is_some()
            || self.mtls_ca_pem.is_some()
            || self.mtls_identity_pem.is_some()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PingResp {
    pub ok: bool,
    pub version: String,
    pub hash: String,
    pub max_chunk_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreUploadStartResp {
    pub upload_id: String,
    pub chunk_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreUploadChunkResp {
    pub ok: bool,
    pub received: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreUploadFinishResp {
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreUploadStatusResp {
    pub received_chunks: Vec<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg(not(target_os = "wasi"))]
struct StoreHasReq<'a> {
    hashes: &'a [String],
}

#[derive(Debug, Clone, Serialize)]
#[cfg(not(target_os = "wasi"))]
struct StoreUploadStartReq<'a> {
    hash: &'a str,
    size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[cfg(not(target_os = "wasi"))]
struct StoreUploadFinishReq<'a> {
    upload_id: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
#[cfg(not(target_os = "wasi"))]
struct StoreHasResp {
    present: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefsGetResp {
    pub name: String,
    pub hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefsListEntry {
    pub name: String,
    pub hash: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[cfg(not(target_os = "wasi"))]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefsSetResp {
    pub ok: bool,
    pub name: String,
    pub hash: String,
}

