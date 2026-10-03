use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;
use std::time::Duration;

use bytes::Bytes;
use futures_util::future::{Either, select};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Request, Response};

use super::Shutdown;
use super::limits::{HttpRegistryServerLimits, limit, reserve};
use super::uploads::UploadState;
use crate::RegistryError;

pub(super) enum Wait<T> {
    Ready(T),
    Stopped,
    Deadline,
}

/// One owned I/O future, one absolute deadline, and bounded session maintenance.
pub(super) async fn owned_io<F: Future>(
    future: F,
    stop: &Shutdown,
    uploads: &Rc<RefCell<UploadState>>,
    limits: &HttpRegistryServerLimits,
    deadline: Duration,
) -> Wait<F::Output> {
    let mut future = std::pin::pin!(future);
    let mut stopped = std::pin::pin!(stop.cancelled());
    let mut timeout = std::pin::pin!(tokio::time::sleep(deadline));
    loop {
        if stop.requested.load(std::sync::atomic::Ordering::Acquire) {
            return Wait::Stopped;
        }
        let tick = tokio::time::sleep(limits.session_lifetime.min(Duration::from_secs(1)));
        let tick = std::pin::pin!(tick);
        match select(
            future.as_mut(),
            select(stopped.as_mut(), select(timeout.as_mut(), tick)),
        )
        .await
        {
            Either::Left((result, _)) => return Wait::Ready(result),
            Either::Right((Either::Left(_), _)) => return Wait::Stopped,
            Either::Right((Either::Right((Either::Left(_), _)), _)) => return Wait::Deadline,
            Either::Right((Either::Right((Either::Right(_), _)), _)) => {
                uploads.borrow_mut().expire(limits);
            }
        }
    }
}

pub(super) async fn read_body(
    mut req: Request<Incoming>,
    bound: usize,
    stop: &Shutdown,
    deadline: tokio::time::Instant,
) -> Result<Vec<u8>, RegistryError> {
    if let Some(raw) = req.headers().get(hyper::header::CONTENT_LENGTH) {
        let declared = raw
            .to_str()
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| RegistryError::Protocol("invalid Content-Length".to_string()))?;
        if declared > bound as u64 {
            return Err(limit("request body exceeds admitted bytes"));
        }
    }
    let mut body = Vec::new();
    while let Some(frame) = req.body_mut().frame().await {
        check_admission(stop, deadline)?;
        let frame = frame.map_err(|e| RegistryError::Http(format!("read body: {e}")))?;
        if let Ok(bytes) = frame.into_data() {
            // Hyper's framing buffer is bounded separately. Never copy an
            // over-budget frame into the application buffer, even without length.
            if bytes.len() > bound - body.len() {
                return Err(limit("request body exceeds admitted bytes"));
            }
            reserve(&mut body, bytes.len(), bound)?;
            body.extend_from_slice(&bytes);
        }
    }
    Ok(body)
}

pub(super) fn check_admission(
    stop: &Shutdown,
    deadline: tokio::time::Instant,
) -> Result<(), RegistryError> {
    if stop.requested.load(std::sync::atomic::Ordering::Acquire) {
        return Err(RegistryError::Http("registry/serve stopped".to_string()));
    }
    if tokio::time::Instant::now() >= deadline {
        return Err(RegistryError::Http(
            "registry/serve request deadline: status 408".to_string(),
        ));
    }
    Ok(())
}

pub(super) fn response(
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::from(body)));
    *response.status_mut() =
        hyper::StatusCode::from_u16(status).unwrap_or(hyper::StatusCode::INTERNAL_SERVER_ERROR);
    response.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        hyper::header::HeaderValue::from_static(content_type),
    );
    response
}
