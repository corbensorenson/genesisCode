impl RegistryClient {
    fn reject_file_transport_auth(&self) -> Result<(), RegistryError> {
        if self.auth.has_any() {
            return Err(RegistryError::Auth(
                "file registry does not support transport auth".to_string(),
            ));
        }
        Ok(())
    }

    fn file_transport_root_for_op(&self, _op: &str) -> Result<Option<PathBuf>, RegistryError> {
        match &self.kind {
            RegistryKind::File { root } => {
                self.reject_file_transport_auth()?;
                Ok(Some(root.clone()))
            }
            #[cfg(target_os = "wasi")]
            RegistryKind::Http => {
                self.reject_file_transport_auth()?;
                let bridge_root = wasi_http_bridge_root_for_base(&self.base)
                    .ok_or_else(|| wasi_http_bridge_required(_op, &self.base))?;
                Ok(Some(bridge_root))
            }
            _ => Ok(None),
        }
    }
}
