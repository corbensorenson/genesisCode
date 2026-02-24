impl RegistryClient {
    pub fn ping(&self) -> Result<PingResp, RegistryError> {
        if let RegistryKind::InProc { id } = &self.kind {
            let g = lock_inproc_map()?;
            let reg = g.get(id).ok_or_else(|| {
                RegistryError::RemoteSpec(format!("inproc registry not registered: {id}"))
            })?;
            reg.authorize(&self.auth)?;
            return reg.ping();
        }
        if let RegistryKind::File { .. } = &self.kind {
            if self.auth.has_any() {
                return Err(RegistryError::Auth(
                    "file registry does not support transport auth".to_string(),
                ));
            }
            return Ok(PingResp {
                ok: true,
                version: "0.1".to_string(),
                hash: "blake3-256".to_string(),
                max_chunk_bytes: None,
            });
        }
        #[cfg(target_os = "wasi")]
        {
            return Err(wasi_http_unsupported("ping"));
        }
        #[cfg(not(target_os = "wasi"))]
        {
            let u = self
                .base
                .join("ping")
                .map_err(|e| RegistryError::RemoteSpec(format!("join ping: {e}")))?;
            let r = self
                .apply_auth(self.http()?.get(u))
                .send()
                .map_err(|e| RegistryError::Http(format!("ping: {e}")))?;
            if !r.status().is_success() {
                return Err(status_error("ping", r.status()));
            }
            r.json::<PingResp>()
                .map_err(|e| RegistryError::Protocol(format!("ping json: {e}")))
        }
    }

    pub fn store_has(&self, hashes: &[String]) -> Result<BTreeMap<String, bool>, RegistryError> {
        if let RegistryKind::InProc { id } = &self.kind {
            let g = lock_inproc_map()?;
            let reg = g.get(id).ok_or_else(|| {
                RegistryError::RemoteSpec(format!("inproc registry not registered: {id}"))
            })?;
            reg.authorize(&self.auth)?;
            return reg.store_has(hashes);
        }
        if let RegistryKind::File { root } = &self.kind {
            if self.auth.has_any() {
                return Err(RegistryError::Auth(
                    "file registry does not support transport auth".to_string(),
                ));
            }
            file_ensure_dirs(root)?;
            let mut out = BTreeMap::new();
            for h in hashes {
                out.insert(h.clone(), file_store_path(root, h).exists());
            }
            return Ok(out);
        }
        #[cfg(target_os = "wasi")]
        {
            return Err(wasi_http_unsupported("store/has"));
        }
        #[cfg(not(target_os = "wasi"))]
        {
            let u = self
                .base
                .join("store/has")
                .map_err(|e| RegistryError::RemoteSpec(format!("join store/has: {e}")))?;
            let r = self
                .apply_auth(self.http()?.post(u))
                .json(&StoreHasReq { hashes })
                .send()
                .map_err(|e| RegistryError::Http(format!("store/has: {e}")))?;
            if !r.status().is_success() {
                return Err(status_error("store/has", r.status()));
            }
            let resp = r
                .json::<StoreHasResp>()
                .map_err(|e| RegistryError::Protocol(format!("store/has json: {e}")))?;
            Ok(resp.present)
        }
    }

    pub fn store_get(&self, hash: &str) -> Result<Vec<u8>, RegistryError> {
        self.store_get_bounded(hash, None)
    }

    pub fn store_get_bounded(
        &self,
        hash: &str,
        max_bytes: Option<usize>,
    ) -> Result<Vec<u8>, RegistryError> {
        if let RegistryKind::InProc { id } = &self.kind {
            let g = lock_inproc_map()?;
            let reg = g.get(id).ok_or_else(|| {
                RegistryError::RemoteSpec(format!("inproc registry not registered: {id}"))
            })?;
            reg.authorize(&self.auth)?;
            let bytes = reg.store_get(hash)?;
            enforce_body_limit("store/get", max_bytes, bytes.len() as u64)?;
            return Ok(bytes);
        }
        if let RegistryKind::File { root } = &self.kind {
            if self.auth.has_any() {
                return Err(RegistryError::Auth(
                    "file registry does not support transport auth".to_string(),
                ));
            }
            file_ensure_dirs(root)?;
            let p = file_store_path(root, hash);
            if !p.exists() {
                return Err(RegistryError::Http("store/get: status 404".to_string()));
            }
            let bytes = std::fs::read(&p).map_err(|e| RegistryError::Http(format!("{e}")))?;
            enforce_body_limit("store/get", max_bytes, bytes.len() as u64)?;
            let got = blake3::hash(&bytes).to_hex().to_string();
            if got != hash {
                return Err(RegistryError::Protocol(
                    "store/get: hash mismatch".to_string(),
                ));
            }
            return Ok(bytes);
        }
        #[cfg(target_os = "wasi")]
        {
            return Err(wasi_http_unsupported("store/get"));
        }
        #[cfg(not(target_os = "wasi"))]
        {
            let u = self
                .base
                .join(&format!("store/get/{hash}"))
                .map_err(|e| RegistryError::RemoteSpec(format!("join store/get: {e}")))?;
            let r = self
                .apply_auth(self.http()?.get(u))
                .send()
                .map_err(|e| RegistryError::Http(format!("store/get: {e}")))?;
            if !r.status().is_success() {
                return Err(status_error("store/get", r.status()));
            }
            read_response_bytes_limited("store/get", r, max_bytes)
        }
    }

    pub fn store_get_opt(&self, hash: &str) -> Result<Option<Vec<u8>>, RegistryError> {
        self.store_get_opt_bounded(hash, None)
    }

    pub fn store_get_opt_bounded(
        &self,
        hash: &str,
        max_bytes: Option<usize>,
    ) -> Result<Option<Vec<u8>>, RegistryError> {
        if let RegistryKind::InProc { id } = &self.kind {
            let g = lock_inproc_map()?;
            let reg = g.get(id).ok_or_else(|| {
                RegistryError::RemoteSpec(format!("inproc registry not registered: {id}"))
            })?;
            reg.authorize(&self.auth)?;
            return match reg.store_get(hash) {
                Ok(b) => {
                    enforce_body_limit("store/get", max_bytes, b.len() as u64)?;
                    Ok(Some(b))
                }
                Err(RegistryError::Http(s)) if s.contains("status 404") => Ok(None),
                Err(e) => Err(e),
            };
        }
        if let RegistryKind::File { root } = &self.kind {
            if self.auth.has_any() {
                return Err(RegistryError::Auth(
                    "file registry does not support transport auth".to_string(),
                ));
            }
            file_ensure_dirs(root)?;
            let p = file_store_path(root, hash);
            if !p.exists() {
                return Ok(None);
            }
            let bytes = std::fs::read(&p).map_err(|e| RegistryError::Http(format!("{e}")))?;
            enforce_body_limit("store/get", max_bytes, bytes.len() as u64)?;
            let got = blake3::hash(&bytes).to_hex().to_string();
            if got != hash {
                return Err(RegistryError::Protocol(
                    "store/get: hash mismatch".to_string(),
                ));
            }
            return Ok(Some(bytes));
        }
        #[cfg(target_os = "wasi")]
        {
            return Err(wasi_http_unsupported("store/get"));
        }
        #[cfg(not(target_os = "wasi"))]
        {
            let u = self
                .base
                .join(&format!("store/get/{hash}"))
                .map_err(|e| RegistryError::RemoteSpec(format!("join store/get: {e}")))?;
            let r = self
                .apply_auth(self.http()?.get(u))
                .send()
                .map_err(|e| RegistryError::Http(format!("store/get: {e}")))?;
            if r.status() == StatusCode::NOT_FOUND {
                return Ok(None);
            }
            if !r.status().is_success() {
                return Err(status_error("store/get", r.status()));
            }
            read_response_bytes_limited("store/get", r, max_bytes).map(Some)
        }
    }

    pub fn store_put(&self, hash: &str, bytes: &[u8]) -> Result<(), RegistryError> {
        if let RegistryKind::InProc { id } = &self.kind {
            let g = lock_inproc_map()?;
            let reg = g.get(id).ok_or_else(|| {
                RegistryError::RemoteSpec(format!("inproc registry not registered: {id}"))
            })?;
            reg.authorize(&self.auth)?;
            return reg.store_put(hash, bytes);
        }
        if let RegistryKind::File { root } = &self.kind {
            if self.auth.has_any() {
                return Err(RegistryError::Auth(
                    "file registry does not support transport auth".to_string(),
                ));
            }
            file_ensure_dirs(root)?;
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
            file_atomic_write(&p, bytes)?;
            return Ok(());
        }
        #[cfg(target_os = "wasi")]
        {
            return Err(wasi_http_unsupported("store/put"));
        }
        #[cfg(not(target_os = "wasi"))]
        {
            let u = self
                .base
                .join(&format!("store/put/{hash}"))
                .map_err(|e| RegistryError::RemoteSpec(format!("join store/put: {e}")))?;
            let r = self
                .apply_auth(self.http()?.put(u))
                .body(bytes.to_vec())
                .send()
                .map_err(|e| RegistryError::Http(format!("store/put: {e}")))?;
            if !r.status().is_success() {
                return Err(status_error("store/put", r.status()));
            }
            Ok(())
        }
    }

    pub fn store_put_auto(
        &self,
        hash: &str,
        bytes: &[u8],
        max_chunk_bytes: Option<usize>,
    ) -> Result<(), RegistryError> {
        let Some(chunk_bytes) = max_chunk_bytes.filter(|n| *n > 0) else {
            return self.store_put(hash, bytes);
        };
        if bytes.len() <= chunk_bytes {
            return self.store_put(hash, bytes);
        }
        match self.store_put_chunked(hash, bytes, chunk_bytes) {
            Ok(()) => Ok(()),
            Err(e) if chunk_upload_not_supported(&e) => self.store_put(hash, bytes),
            Err(e) => Err(e),
        }
    }

    pub fn store_put_chunked(
        &self,
        hash: &str,
        bytes: &[u8],
        chunk_bytes: usize,
    ) -> Result<(), RegistryError> {
        if chunk_bytes == 0 {
            return Err(RegistryError::Protocol(
                "store/upload: chunk size must be > 0".to_string(),
            ));
        }
        let start = self.store_upload_start(hash, bytes.len() as u64)?;
        let chunk_size = usize::try_from(start.chunk_bytes)
            .ok()
            .filter(|n| *n > 0)
            .unwrap_or(chunk_bytes);
        for (idx, chunk) in bytes.chunks(chunk_size).enumerate() {
            self.store_upload_chunk(&start.upload_id, idx as u64, chunk)?;
        }
        let finish = self.store_upload_finish(&start.upload_id)?;
        if !finish.ok {
            return Err(RegistryError::Protocol(
                "store/upload/finish returned ok=false".to_string(),
            ));
        }
        Ok(())
    }

    pub fn store_upload_start(
        &self,
        hash: &str,
        size_bytes: u64,
    ) -> Result<StoreUploadStartResp, RegistryError> {
        if let RegistryKind::InProc { id } = &self.kind {
            let g = lock_inproc_map()?;
            let reg = g.get(id).ok_or_else(|| {
                RegistryError::RemoteSpec(format!("inproc registry not registered: {id}"))
            })?;
            reg.authorize(&self.auth)?;
            return reg.store_upload_start(hash, size_bytes);
        }
        if let RegistryKind::File { .. } = &self.kind {
            return Err(RegistryError::Protocol(
                "store/upload/start: not supported for file:// remotes".to_string(),
            ));
        }
        #[cfg(target_os = "wasi")]
        {
            return Err(wasi_http_unsupported("store/upload/start"));
        }
        #[cfg(not(target_os = "wasi"))]
        {
            let u = self
                .base
                .join("store/upload/start")
                .map_err(|e| RegistryError::RemoteSpec(format!("join store/upload/start: {e}")))?;
            let r = self
                .apply_auth(self.http()?.post(u))
                .json(&StoreUploadStartReq { hash, size_bytes })
                .send()
                .map_err(|e| RegistryError::Http(format!("store/upload/start: {e}")))?;
            if !r.status().is_success() {
                return Err(status_error("store/upload/start", r.status()));
            }
            r.json::<StoreUploadStartResp>()
                .map_err(|e| RegistryError::Protocol(format!("store/upload/start json: {e}")))
        }
    }

    pub fn store_upload_chunk(
        &self,
        upload_id: &str,
        index: u64,
        bytes: &[u8],
    ) -> Result<StoreUploadChunkResp, RegistryError> {
        if let RegistryKind::InProc { id } = &self.kind {
            let g = lock_inproc_map()?;
            let reg = g.get(id).ok_or_else(|| {
                RegistryError::RemoteSpec(format!("inproc registry not registered: {id}"))
            })?;
            reg.authorize(&self.auth)?;
            return reg.store_upload_chunk(upload_id, index, bytes);
        }
        if let RegistryKind::File { .. } = &self.kind {
            return Err(RegistryError::Protocol(
                "store/upload/chunk: not supported for file:// remotes".to_string(),
            ));
        }
        #[cfg(target_os = "wasi")]
        {
            return Err(wasi_http_unsupported("store/upload/chunk"));
        }
        #[cfg(not(target_os = "wasi"))]
        {
            let u = self
                .base
                .join(&format!("store/upload/chunk/{upload_id}/{index}"))
                .map_err(|e| RegistryError::RemoteSpec(format!("join store/upload/chunk: {e}")))?;
            let r = self
                .apply_auth(self.http()?.put(u))
                .body(bytes.to_vec())
                .send()
                .map_err(|e| RegistryError::Http(format!("store/upload/chunk: {e}")))?;
            if !r.status().is_success() {
                return Err(status_error("store/upload/chunk", r.status()));
            }
            r.json::<StoreUploadChunkResp>()
                .map_err(|e| RegistryError::Protocol(format!("store/upload/chunk json: {e}")))
        }
    }

    pub fn store_upload_finish(
        &self,
        upload_id: &str,
    ) -> Result<StoreUploadFinishResp, RegistryError> {
        if let RegistryKind::InProc { id } = &self.kind {
            let g = lock_inproc_map()?;
            let reg = g.get(id).ok_or_else(|| {
                RegistryError::RemoteSpec(format!("inproc registry not registered: {id}"))
            })?;
            reg.authorize(&self.auth)?;
            return reg.store_upload_finish(upload_id);
        }
        if let RegistryKind::File { .. } = &self.kind {
            return Err(RegistryError::Protocol(
                "store/upload/finish: not supported for file:// remotes".to_string(),
            ));
        }
        #[cfg(target_os = "wasi")]
        {
            return Err(wasi_http_unsupported("store/upload/finish"));
        }
        #[cfg(not(target_os = "wasi"))]
        {
            let u = self
                .base
                .join("store/upload/finish")
                .map_err(|e| RegistryError::RemoteSpec(format!("join store/upload/finish: {e}")))?;
            let r = self
                .apply_auth(self.http()?.post(u))
                .json(&StoreUploadFinishReq { upload_id })
                .send()
                .map_err(|e| RegistryError::Http(format!("store/upload/finish: {e}")))?;
            if !r.status().is_success() {
                return Err(status_error("store/upload/finish", r.status()));
            }
            r.json::<StoreUploadFinishResp>()
                .map_err(|e| RegistryError::Protocol(format!("store/upload/finish json: {e}")))
        }
    }

    pub fn store_upload_status(
        &self,
        upload_id: &str,
    ) -> Result<StoreUploadStatusResp, RegistryError> {
        if let RegistryKind::InProc { id } = &self.kind {
            let g = lock_inproc_map()?;
            let reg = g.get(id).ok_or_else(|| {
                RegistryError::RemoteSpec(format!("inproc registry not registered: {id}"))
            })?;
            reg.authorize(&self.auth)?;
            return reg.store_upload_status(upload_id);
        }
        if let RegistryKind::File { .. } = &self.kind {
            return Err(RegistryError::Protocol(
                "store/upload/status: not supported for file:// remotes".to_string(),
            ));
        }
        #[cfg(target_os = "wasi")]
        {
            return Err(wasi_http_unsupported("store/upload/status"));
        }
        #[cfg(not(target_os = "wasi"))]
        {
            let u = self
                .base
                .join(&format!("store/upload/status/{upload_id}"))
                .map_err(|e| RegistryError::RemoteSpec(format!("join store/upload/status: {e}")))?;
            let r = self
                .apply_auth(self.http()?.get(u))
                .send()
                .map_err(|e| RegistryError::Http(format!("store/upload/status: {e}")))?;
            if !r.status().is_success() {
                return Err(status_error("store/upload/status", r.status()));
            }
            r.json::<StoreUploadStatusResp>()
                .map_err(|e| RegistryError::Protocol(format!("store/upload/status json: {e}")))
        }
    }
}
