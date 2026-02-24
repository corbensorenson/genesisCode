impl RegistryClient {
    #[cfg(not(target_os = "wasi"))]
    fn http(&self) -> Result<&Client, RegistryError> {
        match &self.kind {
            RegistryKind::Http { http } => Ok(http),
            RegistryKind::InProc { .. } | RegistryKind::File { .. } => {
                Err(RegistryError::Protocol(
                    "internal registry dispatch drift: HTTP client requested for non-HTTP registry"
                        .to_string(),
                ))
            }
        }
    }

    #[cfg(not(target_os = "wasi"))]
    fn apply_auth(
        &self,
        req: reqwest::blocking::RequestBuilder,
    ) -> reqwest::blocking::RequestBuilder {
        if let Some(token) = &self.auth.bearer_token {
            return req.bearer_auth(token);
        }
        if let Some(user) = &self.auth.basic_username {
            return req.basic_auth(user, self.auth.basic_password.as_deref());
        }
        req
    }
}
