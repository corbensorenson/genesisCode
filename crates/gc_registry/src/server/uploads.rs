use std::collections::BTreeMap;
use std::time::Instant;

use super::limits::{HttpRegistryServerLimits, limit};
use crate::RegistryError;

#[derive(Debug)]
struct UploadSession {
    hash: String,
    size: usize,
    bytes: usize,
    created: Instant,
    chunks: BTreeMap<u64, Vec<u8>>,
}

#[derive(Debug, Default)]
pub(super) struct UploadState {
    next_id: u64,
    reserved: usize,
    retained: usize,
    sessions: BTreeMap<String, UploadSession>,
}

impl UploadState {
    pub(super) fn expire(&mut self, limits: &HttpRegistryServerLimits) {
        let now = Instant::now();
        self.sessions.retain(|_, session| {
            if now.duration_since(session.created) < limits.session_lifetime {
                true
            } else {
                self.reserved -= session.size;
                self.retained -= session.bytes;
                false
            }
        });
    }

    pub(super) fn start(
        &mut self,
        hash: String,
        size: u64,
        chunk_bytes: u64,
        limits: &HttpRegistryServerLimits,
    ) -> Result<String, RegistryError> {
        crate::validate_store_hash(&hash)?;
        self.expire(limits);
        let size = usize::try_from(size).map_err(|_| limit("upload size is not representable"))?;
        if size > limits.max_object_bytes {
            return Err(limit("upload object size exceeded"));
        }
        let minimum_chunks = (size as u64).div_ceil(chunk_bytes);
        if minimum_chunks > limits.max_chunks_per_session as u64 {
            return Err(limit("upload requires too many chunks"));
        }
        let reserved = self
            .reserved
            .checked_add(size)
            .ok_or_else(|| limit("upload reservation overflow"))?;
        if self.sessions.len() >= limits.max_sessions || reserved > limits.max_upload_bytes {
            return Err(limit("upload session/reservation budget exhausted"));
        }
        let id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| limit("upload identity exhausted"))?;
        let name = format!("u_{id}");
        self.sessions.insert(
            name.clone(),
            UploadSession {
                hash,
                size,
                bytes: 0,
                created: Instant::now(),
                chunks: BTreeMap::new(),
            },
        );
        self.next_id = id;
        self.reserved = reserved;
        Ok(name)
    }

    pub(super) fn insert(
        &mut self,
        id: &str,
        index: u64,
        bytes: Vec<u8>,
        limits: &HttpRegistryServerLimits,
    ) -> Result<usize, RegistryError> {
        self.expire(limits);
        let session = self.sessions.get_mut(id).ok_or_else(|| missing("chunk"))?;
        if index >= limits.max_chunks_per_session as u64 {
            return Err(limit("upload chunk index exceeded"));
        }
        let previous = session.chunks.get(&index).map_or(0, Vec::len);
        let total = (session.bytes - previous)
            .checked_add(bytes.len())
            .ok_or_else(|| limit("upload accounting overflow"))?;
        let retained = (self.retained - previous)
            .checked_add(bytes.len())
            .ok_or_else(|| limit("upload accounting overflow"))?;
        if total > session.size || retained > limits.max_upload_bytes {
            return Err(limit("upload retained byte budget exceeded"));
        }
        let received = bytes.len();
        session.chunks.insert(index, bytes);
        session.bytes = total;
        self.retained = retained;
        Ok(received)
    }

    pub(super) fn body_limit(
        &mut self,
        id: &str,
        index: u64,
        chunk_bytes: usize,
        limits: &HttpRegistryServerLimits,
    ) -> Result<usize, RegistryError> {
        self.expire(limits);
        if index >= limits.max_chunks_per_session as u64 {
            return Err(limit("upload chunk index exceeded"));
        }
        let session = self.sessions.get(id).ok_or_else(|| missing("chunk"))?;
        let previous = session.chunks.get(&index).map_or(0, Vec::len);
        Ok(chunk_bytes
            .min(session.size - (session.bytes - previous))
            .min(limits.max_upload_bytes - (self.retained - previous)))
    }

    pub(super) fn status(
        &mut self,
        id: &str,
        limits: &HttpRegistryServerLimits,
    ) -> Result<Vec<u64>, RegistryError> {
        self.expire(limits);
        let session = self.sessions.get(id).ok_or_else(|| missing("status"))?;
        Ok(session.chunks.keys().copied().collect())
    }

    pub(super) fn finish(
        &mut self,
        id: &str,
        limits: &HttpRegistryServerLimits,
    ) -> Result<(String, Vec<u8>), RegistryError> {
        self.expire(limits);
        // Failed finish is terminal, as in the previous wire contract.
        let session = self.sessions.remove(id).ok_or_else(|| missing("finish"))?;
        self.reserved -= session.size;
        self.retained -= session.bytes;
        if session.bytes != session.size {
            return Err(RegistryError::Protocol(
                "store/upload/finish: size mismatch".to_string(),
            ));
        }
        for (expected, &index) in session.chunks.keys().enumerate() {
            if index != expected as u64 {
                return Err(RegistryError::Protocol(
                    "store/upload/finish: missing chunk index".to_string(),
                ));
            }
        }
        let mut payload = Vec::new();
        payload
            .try_reserve_exact(session.size)
            .map_err(|_| limit("upload assembly allocation failed"))?;
        for chunk in session.chunks.into_values() {
            payload.extend_from_slice(&chunk);
        }
        Ok((session.hash, payload))
    }
}

fn missing(operation: &str) -> RegistryError {
    RegistryError::Http(format!("store/upload/{operation}: status 404"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn upload_accounting_preserves_rejections_and_releases_owned_bytes() {
        let limits = HttpRegistryServerLimits {
            max_object_bytes: 4,
            max_upload_bytes: 8,
            max_sessions: 2,
            max_chunks_per_session: 2,
            session_lifetime: Duration::from_secs(300),
            ..HttpRegistryServerLimits::default()
        };
        let mut state = UploadState::default();
        let first = state.start("0".repeat(64), 4, 4, &limits).unwrap();
        let second = state.start("1".repeat(64), 4, 4, &limits).unwrap();
        assert_eq!(
            (state.reserved, state.retained, state.sessions.len()),
            (8, 0, 2)
        );
        assert!(state.start("2".repeat(64), 0, 4, &limits).is_err());
        assert_eq!(
            (state.next_id, state.reserved, state.sessions.len()),
            (2, 8, 2)
        );
        state.insert(&first, 0, b"1234".to_vec(), &limits).unwrap();
        state.insert(&second, 0, b"abcd".to_vec(), &limits).unwrap();
        assert_eq!((state.reserved, state.retained), (8, 8));
        assert_eq!(state.body_limit(&first, 0, 4, &limits).unwrap(), 4);
        assert_eq!(state.body_limit(&first, 1, 4, &limits).unwrap(), 0);
        assert!(state.insert(&first, 1, b"x".to_vec(), &limits).is_err());
        assert_eq!((state.reserved, state.retained), (8, 8));
        state.insert(&first, 0, b"12".to_vec(), &limits).unwrap();
        assert_eq!((state.reserved, state.retained), (8, 6));
        state.insert(&first, 1, b"34".to_vec(), &limits).unwrap();
        let (_, payload) = state.finish(&first, &limits).unwrap();
        assert_eq!(payload, b"1234");
        assert_eq!((state.reserved, state.retained), (4, 4));
        state.sessions.get_mut(&second).unwrap().created -= Duration::from_secs(301);
        state.expire(&limits);
        assert_eq!(
            (state.reserved, state.retained, state.sessions.len()),
            (0, 0, 0)
        );
    }

    #[test]
    fn sparse_or_incomplete_finish_is_terminal_without_assembly() {
        let limits = HttpRegistryServerLimits::default();
        let mut state = UploadState::default();
        for (index, bytes) in [(1, b"x".to_vec()), (0, Vec::new())] {
            let id = state.start("0".repeat(64), 1, 1, &limits).unwrap();
            state.insert(&id, index, bytes, &limits).unwrap();
            assert!(state.finish(&id, &limits).is_err());
            assert_eq!(
                (state.reserved, state.retained, state.sessions.len()),
                (0, 0, 0)
            );
        }
    }
}
