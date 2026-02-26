impl RegistryClient {
    pub fn refs_get(&self, name: &str) -> Result<Option<String>, RegistryError> {
        if let RegistryKind::InProc { id } = &self.kind {
            let g = lock_inproc_map()?;
            let reg = g.get(id).ok_or_else(|| {
                RegistryError::RemoteSpec(format!("inproc registry not registered: {id}"))
            })?;
            reg.authorize(&self.auth)?;
            return reg.refs_get(name);
        }
        if let Some(root) = self.file_transport_root_for_op("refs/get")? {
            file_ensure_dirs(&root)?;
            let _lk = file_refs_lock(&root)?;
            let refs = file_load_refs_locked(&root)?;
            return Ok(refs.get(name).cloned());
        }
        #[cfg(target_os = "wasi")]
        {
            return Err(wasi_http_bridge_required("refs/get", &self.base));
        }
        #[cfg(not(target_os = "wasi"))]
        {
            let mut u = self
                .base
                .join("refs/get")
                .map_err(|e| RegistryError::RemoteSpec(format!("join refs/get: {e}")))?;
            u.query_pairs_mut().append_pair("name", name);
            let r = self
                .apply_auth(self.http()?.get(u))
                .send()
                .map_err(|e| RegistryError::Http(format!("refs/get: {e}")))?;
            if !r.status().is_success() {
                return Err(status_error("refs/get", r.status()));
            }
            let resp = r
                .json::<RefsGetResp>()
                .map_err(|e| RegistryError::Protocol(format!("refs/get json: {e}")))?;
            Ok(resp.hash)
        }
    }

    pub fn refs_list(&self, prefix: Option<&str>) -> Result<Vec<RefsListEntry>, RegistryError> {
        if let RegistryKind::InProc { id } = &self.kind {
            let g = lock_inproc_map()?;
            let reg = g.get(id).ok_or_else(|| {
                RegistryError::RemoteSpec(format!("inproc registry not registered: {id}"))
            })?;
            reg.authorize(&self.auth)?;
            return reg.refs_list(prefix);
        }
        if let Some(root) = self.file_transport_root_for_op("refs/list")? {
            file_ensure_dirs(&root)?;
            let _lk = file_refs_lock(&root)?;
            let refs = file_load_refs_locked(&root)?;
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
        #[cfg(target_os = "wasi")]
        {
            return Err(wasi_http_bridge_required("refs/list", &self.base));
        }
        #[cfg(not(target_os = "wasi"))]
        {
            let mut u = self
                .base
                .join("refs/list")
                .map_err(|e| RegistryError::RemoteSpec(format!("join refs/list: {e}")))?;
            if let Some(p) = prefix {
                u.query_pairs_mut().append_pair("prefix", p);
            }
            let r = self
                .apply_auth(self.http()?.get(u))
                .send()
                .map_err(|e| RegistryError::Http(format!("refs/list: {e}")))?;
            if !r.status().is_success() {
                return Err(status_error("refs/list", r.status()));
            }
            let resp = r
                .json::<RefsListResp>()
                .map_err(|e| RegistryError::Protocol(format!("refs/list json: {e}")))?;
            Ok(resp.refs)
        }
    }

    pub fn refs_set(&self, req: &RefsSetReq<'_>) -> Result<RefsSetResp, RegistryError> {
        if let RegistryKind::InProc { id } = &self.kind {
            let g = lock_inproc_map()?;
            let reg = g.get(id).ok_or_else(|| {
                RegistryError::RemoteSpec(format!("inproc registry not registered: {id}"))
            })?;
            reg.authorize(&self.auth)?;
            return reg.refs_set(req);
        }
        if let Some(root) = self.file_transport_root_for_op("refs/set")? {
            file_ensure_dirs(&root)?;
            let mut lk = file_refs_lock(&root)?;
            let mut refs = file_load_refs_locked(&root)?;

            file_gate_refs_set(&root, req.name, req.hash, req.policy)?;

            let cur = refs.get(req.name).cloned();
            if let Some(exp) = req.expected_old
                && cur.as_deref() != Some(exp)
            {
                return Err(RegistryError::Http("refs/set: status 409".to_string()));
            }
            refs.insert(req.name.to_string(), req.hash.to_string());
            file_write_refs_locked(&root, &refs, &mut lk)?;
            return Ok(RefsSetResp {
                ok: true,
                name: req.name.to_string(),
                hash: req.hash.to_string(),
            });
        }
        #[cfg(target_os = "wasi")]
        {
            return Err(wasi_http_bridge_required("refs/set", &self.base));
        }
        #[cfg(not(target_os = "wasi"))]
        {
            let u = self
                .base
                .join("refs/set")
                .map_err(|e| RegistryError::RemoteSpec(format!("join refs/set: {e}")))?;
            let r = self
                .apply_auth(self.http()?.post(u))
                .json(req)
                .send()
                .map_err(|e| RegistryError::Http(format!("refs/set: {e}")))?;
            if !r.status().is_success() {
                return Err(status_error("refs/set", r.status()));
            }
            r.json::<RefsSetResp>()
                .map_err(|e| RegistryError::Protocol(format!("refs/set json: {e}")))
        }
    }
}
