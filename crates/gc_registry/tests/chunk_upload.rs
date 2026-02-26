use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use gc_registry::{
    InProcRegistry, PingResp, RefsListEntry, RefsSetReq, RefsSetResp, RegistryClient,
    RegistryError, StoreUploadChunkResp, StoreUploadFinishResp, StoreUploadStartResp,
    StoreUploadStatusResp,
};

fn hash_bytes_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn make_temp_dir(prefix: &str) -> std::path::PathBuf {
    let mut dir = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock drift")
        .as_nanos();
    dir.push(format!(
        "genesis-gc-registry-{prefix}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("mkdir temp test dir");
    dir
}

#[derive(Debug)]
struct UploadSession {
    hash: String,
    size_bytes: u64,
    chunk_bytes: u64,
    chunks: BTreeMap<u64, Vec<u8>>,
}

#[derive(Debug)]
struct ChunkRegistry {
    chunk_bytes: u64,
    store: Mutex<BTreeMap<String, Vec<u8>>>,
    uploads: Mutex<BTreeMap<String, UploadSession>>,
    next_upload_id: AtomicU64,
    upload_starts: AtomicU64,
    upload_chunks: AtomicU64,
    upload_finishes: AtomicU64,
}

impl ChunkRegistry {
    fn new(chunk_bytes: u64) -> Self {
        Self {
            chunk_bytes,
            store: Mutex::new(BTreeMap::new()),
            uploads: Mutex::new(BTreeMap::new()),
            next_upload_id: AtomicU64::new(1),
            upload_starts: AtomicU64::new(0),
            upload_chunks: AtomicU64::new(0),
            upload_finishes: AtomicU64::new(0),
        }
    }

    fn counters(&self) -> (u64, u64, u64) {
        (
            self.upload_starts.load(Ordering::Relaxed),
            self.upload_chunks.load(Ordering::Relaxed),
            self.upload_finishes.load(Ordering::Relaxed),
        )
    }
}

impl InProcRegistry for ChunkRegistry {
    fn ping(&self) -> Result<PingResp, RegistryError> {
        Ok(PingResp {
            ok: true,
            version: "0.1".to_string(),
            hash: "blake3-256".to_string(),
            max_chunk_bytes: Some(self.chunk_bytes),
        })
    }

    fn store_has(&self, hashes: &[String]) -> Result<BTreeMap<String, bool>, RegistryError> {
        let g = self.store.lock().expect("lock");
        let mut out = BTreeMap::new();
        for h in hashes {
            out.insert(h.clone(), g.contains_key(h));
        }
        Ok(out)
    }

    fn store_get(&self, hash: &str) -> Result<Vec<u8>, RegistryError> {
        let g = self.store.lock().expect("lock");
        g.get(hash)
            .cloned()
            .ok_or_else(|| RegistryError::Http("store/get: status 404".to_string()))
    }

    fn store_put(&self, hash: &str, bytes: &[u8]) -> Result<(), RegistryError> {
        let got = hash_bytes_hex(bytes);
        if got != hash {
            return Err(RegistryError::Protocol(
                "store/put: hash mismatch".to_string(),
            ));
        }
        let mut g = self.store.lock().expect("lock");
        g.entry(hash.to_string()).or_insert_with(|| bytes.to_vec());
        Ok(())
    }

    fn store_upload_start(
        &self,
        hash: &str,
        size_bytes: u64,
    ) -> Result<StoreUploadStartResp, RegistryError> {
        self.upload_starts.fetch_add(1, Ordering::Relaxed);
        let upload_id = format!("u_{}", self.next_upload_id.fetch_add(1, Ordering::Relaxed));
        let session = UploadSession {
            hash: hash.to_string(),
            size_bytes,
            chunk_bytes: self.chunk_bytes,
            chunks: BTreeMap::new(),
        };
        let mut g = self.uploads.lock().expect("lock");
        g.insert(upload_id.clone(), session);
        Ok(StoreUploadStartResp {
            upload_id,
            chunk_bytes: self.chunk_bytes,
        })
    }

    fn store_upload_chunk(
        &self,
        upload_id: &str,
        index: u64,
        bytes: &[u8],
    ) -> Result<StoreUploadChunkResp, RegistryError> {
        self.upload_chunks.fetch_add(1, Ordering::Relaxed);
        let mut g = self.uploads.lock().expect("lock");
        let session = g
            .get_mut(upload_id)
            .ok_or_else(|| RegistryError::Http("store/upload/chunk: status 404".to_string()))?;
        if bytes.len() as u64 > session.chunk_bytes {
            return Err(RegistryError::Protocol(
                "store/upload/chunk: chunk exceeds advertised chunk_bytes".to_string(),
            ));
        }
        session.chunks.insert(index, bytes.to_vec());
        Ok(StoreUploadChunkResp {
            ok: true,
            received: bytes.len() as u64,
        })
    }

    fn store_upload_finish(&self, upload_id: &str) -> Result<StoreUploadFinishResp, RegistryError> {
        self.upload_finishes.fetch_add(1, Ordering::Relaxed);
        let session = {
            let mut g = self.uploads.lock().expect("lock");
            g.remove(upload_id)
                .ok_or_else(|| RegistryError::Http("store/upload/finish: status 404".to_string()))?
        };
        let mut keys: Vec<u64> = session.chunks.keys().copied().collect();
        keys.sort_unstable();
        for (i, k) in keys.iter().enumerate() {
            if *k != i as u64 {
                return Err(RegistryError::Protocol(
                    "store/upload/finish: missing chunk index".to_string(),
                ));
            }
        }
        let mut assembled = Vec::new();
        for i in 0..keys.len() as u64 {
            let chunk = session.chunks.get(&i).expect("chunk must exist");
            assembled.extend_from_slice(chunk);
        }
        if assembled.len() as u64 != session.size_bytes {
            return Err(RegistryError::Protocol(
                "store/upload/finish: size mismatch".to_string(),
            ));
        }
        let got = hash_bytes_hex(&assembled);
        if got != session.hash {
            return Err(RegistryError::Protocol(
                "store/upload/finish: hash mismatch".to_string(),
            ));
        }
        self.store_put(&session.hash, &assembled)?;
        Ok(StoreUploadFinishResp { ok: true })
    }

    fn store_upload_status(&self, upload_id: &str) -> Result<StoreUploadStatusResp, RegistryError> {
        let g = self.uploads.lock().expect("lock");
        let session = g
            .get(upload_id)
            .ok_or_else(|| RegistryError::Http("store/upload/status: status 404".to_string()))?;
        let mut received_chunks: Vec<u64> = session.chunks.keys().copied().collect();
        received_chunks.sort_unstable();
        Ok(StoreUploadStatusResp { received_chunks })
    }

    fn refs_get(&self, _name: &str) -> Result<Option<String>, RegistryError> {
        Ok(None)
    }

    fn refs_list(&self, _prefix: Option<&str>) -> Result<Vec<RefsListEntry>, RegistryError> {
        Ok(Vec::new())
    }

    fn refs_set(&self, req: &RefsSetReq<'_>) -> Result<RefsSetResp, RegistryError> {
        Ok(RefsSetResp {
            ok: true,
            name: req.name.to_string(),
            hash: req.hash.to_string(),
        })
    }
}

#[test]
fn adaptive_upload_uses_chunked_path_when_payload_exceeds_limit() {
    let reg = Arc::new(ChunkRegistry::new(8));
    gc_registry::register_inproc("chunk_adaptive", reg.clone()).expect("register inproc");

    let client =
        RegistryClient::new("inproc://chunk_adaptive/", None).expect("create registry client");
    let payload = b"abcdefghijklmnopqrstuvwxyz0123456789".to_vec();
    let hash = hash_bytes_hex(&payload);

    client
        .store_put_auto(&hash, &payload, Some(8))
        .expect("chunked put should succeed");

    let got = client.store_get(&hash).expect("store_get");
    assert_eq!(got, payload);

    let (starts, chunks, finishes) = reg.counters();
    assert_eq!(starts, 1);
    assert!(chunks >= 2);
    assert_eq!(finishes, 1);

    gc_registry::unregister_inproc("chunk_adaptive").expect("unregister inproc");
}

#[test]
fn chunk_upload_status_supports_resume() {
    let reg = Arc::new(ChunkRegistry::new(8));
    gc_registry::register_inproc("chunk_resume", reg.clone()).expect("register inproc");

    let client = RegistryClient::new("inproc://chunk_resume/", None).expect("create client");
    let payload = b"abcdefghijklmnopqrstuvwxyz0123456789".to_vec();
    let hash = hash_bytes_hex(&payload);

    let start = client
        .store_upload_start(&hash, payload.len() as u64)
        .expect("start upload");
    client
        .store_upload_chunk(&start.upload_id, 0, &payload[0..8])
        .expect("chunk 0");

    let st = client
        .store_upload_status(&start.upload_id)
        .expect("upload status");
    assert_eq!(st.received_chunks, vec![0]);

    let client2 = RegistryClient::new("inproc://chunk_resume/", None).expect("create client2");
    client2
        .store_upload_chunk(&start.upload_id, 1, &payload[8..16])
        .expect("chunk 1");
    client2
        .store_upload_chunk(&start.upload_id, 2, &payload[16..24])
        .expect("chunk 2");
    client2
        .store_upload_chunk(&start.upload_id, 3, &payload[24..32])
        .expect("chunk 3");
    client2
        .store_upload_chunk(&start.upload_id, 4, &payload[32..])
        .expect("chunk 4");
    client2
        .store_upload_finish(&start.upload_id)
        .expect("finish upload");

    let got = client2.store_get(&hash).expect("store_get");
    assert_eq!(got, payload);

    gc_registry::unregister_inproc("chunk_resume").expect("unregister inproc");
}

#[test]
fn chunk_upload_finish_hash_mismatch_fails_close() {
    let reg = Arc::new(ChunkRegistry::new(8));
    gc_registry::register_inproc("chunk_mismatch", reg).expect("register inproc");

    let client = RegistryClient::new("inproc://chunk_mismatch/", None).expect("create client");
    let payload = b"abcdefghijklmnopqrstuvwxyz0123456789".to_vec();
    let hash = hash_bytes_hex(&payload);
    let mut tampered = payload.clone();
    tampered[3] ^= 0xff;

    let start = client
        .store_upload_start(&hash, tampered.len() as u64)
        .expect("start upload");
    for (idx, chunk) in tampered.chunks(8).enumerate() {
        client
            .store_upload_chunk(&start.upload_id, idx as u64, chunk)
            .expect("upload chunk");
    }

    let err = client
        .store_upload_finish(&start.upload_id)
        .expect_err("finish should fail for hash mismatch");
    assert!(format!("{err}").contains("hash mismatch"));

    let got = client.store_get_opt(&hash).expect("store_get_opt");
    assert!(got.is_none(), "mismatched upload must not store bytes");

    gc_registry::unregister_inproc("chunk_mismatch").expect("unregister inproc");
}

#[test]
fn chunk_upload_roundtrip_over_file_remote() {
    let test_root = make_temp_dir("file-chunk-roundtrip");
    let remote_dir = test_root.join("remote");
    std::fs::create_dir_all(&remote_dir).expect("mkdir");
    let remote = format!("file://{}/", remote_dir.display());

    let client = RegistryClient::new(&remote, None).expect("create file client");
    let payload = b"abcdefghijklmnopqrstuvwxyz0123456789".to_vec();
    let hash = hash_bytes_hex(&payload);

    client
        .store_put_chunked(&hash, &payload, 8)
        .expect("chunked file put should succeed");

    let got = client.store_get(&hash).expect("store_get");
    assert_eq!(got, payload);
    std::fs::remove_dir_all(&test_root).expect("cleanup");
}

#[test]
fn chunk_upload_status_resume_over_file_remote() {
    let test_root = make_temp_dir("file-chunk-resume");
    let remote_dir = test_root.join("remote");
    std::fs::create_dir_all(&remote_dir).expect("mkdir");
    let remote = format!("file://{}/", remote_dir.display());

    let client = RegistryClient::new(&remote, None).expect("create file client");
    let payload = b"abcdefghijklmnopqrstuvwxyz0123456789".to_vec();
    let hash = hash_bytes_hex(&payload);

    let start = client
        .store_upload_start(&hash, payload.len() as u64)
        .expect("start upload");
    client
        .store_upload_chunk(&start.upload_id, 0, &payload[0..8])
        .expect("chunk 0");

    let st = client
        .store_upload_status(&start.upload_id)
        .expect("upload status");
    assert_eq!(st.received_chunks, vec![0]);

    let client2 = RegistryClient::new(&remote, None).expect("create client2");
    client2
        .store_upload_chunk(&start.upload_id, 1, &payload[8..16])
        .expect("chunk 1");
    client2
        .store_upload_chunk(&start.upload_id, 2, &payload[16..24])
        .expect("chunk 2");
    client2
        .store_upload_chunk(&start.upload_id, 3, &payload[24..32])
        .expect("chunk 3");
    client2
        .store_upload_chunk(&start.upload_id, 4, &payload[32..])
        .expect("chunk 4");
    client2
        .store_upload_finish(&start.upload_id)
        .expect("finish upload");

    let got = client2.store_get(&hash).expect("store_get");
    assert_eq!(got, payload);
    std::fs::remove_dir_all(&test_root).expect("cleanup");
}
