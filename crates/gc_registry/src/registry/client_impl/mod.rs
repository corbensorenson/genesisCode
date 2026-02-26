impl RegistryClient {
    pub fn new(remote: &str, timeout: Option<Duration>) -> Result<Self, RegistryError> {
        Self::new_with_auth(remote, timeout, RegistryAuth::default())
    }

    pub fn new_with_auth(
        remote: &str,
        timeout: Option<Duration>,
        auth: RegistryAuth,
    ) -> Result<Self, RegistryError> {
        if auth.bearer_token.is_some() && auth.basic_username.is_some() {
            return Err(RegistryError::Auth(
                "bearer and basic auth are mutually exclusive".to_string(),
            ));
        }
        if auth.basic_username.is_none() && auth.basic_password.is_some() {
            return Err(RegistryError::Auth(
                "basic auth password requires basic username".to_string(),
            ));
        }
        #[cfg(target_os = "wasi")]
        let _ = timeout;
        let base = normalize_remote_base(remote)?;
        let kind = match base.scheme() {
            "https" | "http" => {
                if let Some(root) = wasi_http_bridge_root_for_base(&base) {
                    if auth.has_any() {
                        return Err(RegistryError::Auth(format!(
                            "http bridge adapter does not support registry transport auth; unset auth fields or clear {WASI_HTTP_BRIDGE_ROOT_ENV}"
                        )));
                    }
                    RegistryKind::File { root }
                } else {
                    #[cfg(target_os = "wasi")]
                    {
                        if auth.has_any() {
                            return Err(RegistryError::Auth(
                                "http(s) registry auth is not supported on WASI builds".to_string(),
                            ));
                        }
                        RegistryKind::Http
                    }
                    #[cfg(not(target_os = "wasi"))]
                    {
                        let mut b = Client::builder();
                        if let Some(t) = timeout {
                            b = b.timeout(t);
                        }
                        if let Some(ca_pem) = auth.mtls_ca_pem.as_ref() {
                            let cert = reqwest::Certificate::from_pem(ca_pem).map_err(|e| {
                                RegistryError::Auth(format!("invalid mTLS CA PEM: {e}"))
                            })?;
                            b = b.add_root_certificate(cert);
                        }
                        if let Some(identity_pem) = auth.mtls_identity_pem.as_ref() {
                            let identity =
                                reqwest::Identity::from_pem(identity_pem).map_err(|e| {
                                    RegistryError::Auth(format!("invalid mTLS identity PEM: {e}"))
                                })?;
                            b = b.identity(identity);
                        }
                        let http = b
                            .build()
                            .map_err(|e| RegistryError::Http(format!("build client: {e}")))?;
                        RegistryKind::Http { http }
                    }
                }
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
        Ok(Self { base, kind, auth })
    }

    pub fn base_url(&self) -> &Url {
        &self.base
    }
}

include!("file_transport.rs");
include!("ping_and_store.rs");
include!("refs.rs");
include!("auth.rs");
