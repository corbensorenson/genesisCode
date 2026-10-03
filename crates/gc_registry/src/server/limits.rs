use std::time::Duration;

use crate::RegistryError;

/// Native registry resource admission. Callers may select smaller finite bounds.
#[derive(Debug, Clone)]
pub struct HttpRegistryServerLimits {
    pub max_json_bytes: usize,
    pub max_object_bytes: usize,
    pub max_upload_bytes: usize,
    pub max_sessions: usize,
    pub max_chunks_per_session: usize,
    pub request_timeout: Duration,
    pub session_lifetime: Duration,
}

impl Default for HttpRegistryServerLimits {
    fn default() -> Self {
        Self {
            max_json_bytes: 1 << 20,
            max_object_bytes: 64 << 20,
            max_upload_bytes: 128 << 20,
            max_sessions: 32,
            max_chunks_per_session: 1024,
            request_timeout: Duration::from_secs(30),
            session_lifetime: Duration::from_secs(300),
        }
    }
}

impl HttpRegistryServerLimits {
    pub(super) fn validate(&self, chunk_bytes: u64) -> Result<(), RegistryError> {
        let ceiling = Self::default();
        if self.max_json_bytes < 256 {
            return Err(limit("JSON bound cannot carry the protocol envelope"));
        }
        for (value, maximum) in [
            (self.max_json_bytes, ceiling.max_json_bytes),
            (self.max_object_bytes, ceiling.max_object_bytes),
            (self.max_upload_bytes, ceiling.max_upload_bytes),
            (self.max_sessions, ceiling.max_sessions),
            (self.max_chunks_per_session, ceiling.max_chunks_per_session),
        ] {
            if value == 0 || value > maximum {
                return Err(limit("invalid server resource policy"));
            }
        }
        if chunk_bytes == 0 || chunk_bytes > self.max_object_bytes as u64 {
            return Err(limit("invalid maximum chunk size"));
        }
        if self.max_upload_bytes < self.max_object_bytes {
            return Err(limit("upload budget is smaller than the object bound"));
        }
        if self.request_timeout.is_zero()
            || self.request_timeout > ceiling.request_timeout
            || self.session_lifetime.is_zero()
            || self.session_lifetime > ceiling.session_lifetime
        {
            return Err(limit("invalid server lifetime policy"));
        }
        Ok(())
    }
}

pub(super) fn limit(message: &str) -> RegistryError {
    RegistryError::Protocol(format!("resource-limit: registry/serve {message}"))
}

pub(super) fn reserve(
    bytes: &mut Vec<u8>,
    growth: usize,
    bound: usize,
) -> Result<(), RegistryError> {
    let needed = bytes
        .len()
        .checked_add(growth)
        .ok_or_else(|| limit("buffer size overflow"))?;
    if needed > bound {
        return Err(limit("buffer bytes exceeded"));
    }
    if needed > bytes.capacity() {
        let capacity = needed
            .max(bytes.capacity().saturating_mul(2))
            .max(4096.min(bound))
            .min(bound);
        bytes
            .try_reserve_exact(capacity - bytes.len())
            .map_err(|_| limit("buffer allocation failed"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn buffer_growth_is_bounded_geometric_and_refuses_before_allocation() {
        let mut bytes = Vec::new();
        for _ in 0..10000 {
            reserve(&mut bytes, 1, 10000).unwrap();
            bytes.push(0);
        }
        assert_eq!(bytes.len(), 10000);
        assert!(bytes.capacity() <= 10000);
        let before = (bytes.len(), bytes.capacity());
        assert!(reserve(&mut bytes, 1, 10000).is_err());
        assert!(reserve(&mut bytes, usize::MAX, 10000).is_err());
        assert_eq!((bytes.len(), bytes.capacity()), before);
    }
}
