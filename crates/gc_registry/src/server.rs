use std::cell::{Cell, RefCell};
use std::convert::Infallible;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use hyper::service::service_fn;
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::sync::Notify;

use crate::{RegistryClient, RegistryError};
mod limits;
mod requests;
mod transport;
mod uploads;
pub use limits::HttpRegistryServerLimits;
use transport::Wait;
use uploads::UploadState;

#[derive(Debug, Clone)]
pub struct HttpRegistryServerConfig {
    pub addr: String,
    pub root: PathBuf,
    pub max_chunk_bytes: u64,
    pub max_requests: Option<u64>,
}
impl Default for HttpRegistryServerConfig {
    fn default() -> Self {
        Self {
            addr: "127.0.0.1:0".to_string(),
            root: PathBuf::from("."),
            max_chunk_bytes: 4_194_304,
            max_requests: None,
        }
    }
}

#[derive(Debug, Default)]
struct Shutdown {
    requested: AtomicBool,
    wake: Notify,
}
impl Shutdown {
    fn stop(&self) {
        self.requested.store(true, Ordering::Release);
        self.wake.notify_one();
    }
    async fn cancelled(&self) {
        let notified = self.wake.notified();
        if self.requested.load(Ordering::Acquire) {
            return;
        }
        notified.await;
    }
}

/// A cloneable stop signal remains usable while the owner naturally waits.
#[derive(Debug, Clone)]
pub struct HttpRegistryShutdown {
    inner: Arc<Shutdown>,
}
impl HttpRegistryShutdown {
    pub fn shutdown(&self) {
        self.inner.stop();
    }
}

#[derive(Debug)]
pub struct HttpRegistryServerHandle {
    shutdown: Arc<Shutdown>,
    join: Option<thread::JoinHandle<Result<(), RegistryError>>>,
    bound_addr: String,
}
impl HttpRegistryServerHandle {
    pub fn bound_addr(&self) -> &str {
        &self.bound_addr
    }
    pub fn shutdown_handle(&self) -> HttpRegistryShutdown {
        HttpRegistryShutdown {
            inner: self.shutdown.clone(),
        }
    }
    pub fn shutdown(&self) {
        self.shutdown.stop();
    }
    /// Wait for the configured stop policy; this does not request a stop.
    pub fn join(mut self) -> Result<(), RegistryError> {
        self.wait_worker()
    }
    pub fn stop_and_join(mut self) -> Result<(), RegistryError> {
        self.shutdown();
        self.wait_worker()
    }
    fn wait_worker(&mut self) -> Result<(), RegistryError> {
        match self.join.take() {
            Some(worker) => worker.join().map_err(|_| {
                RegistryError::Protocol("registry server thread panicked".to_string())
            })?,
            None => Ok(()),
        }
    }
}
impl Drop for HttpRegistryServerHandle {
    fn drop(&mut self) {
        if self.join.is_some() {
            self.shutdown();
            // Drop cannot return an error; explicit join methods retain diagnostics.
            // No server or connection thread is detached on this path.
            let _ = self.wait_worker();
        }
    }
}

pub fn spawn_http_file_registry_server(
    cfg: HttpRegistryServerConfig,
) -> Result<HttpRegistryServerHandle, RegistryError> {
    spawn_http_file_registry_server_with_limits(cfg, HttpRegistryServerLimits::default())
}
pub fn spawn_http_file_registry_server_with_limits(
    cfg: HttpRegistryServerConfig,
    limits: HttpRegistryServerLimits,
) -> Result<HttpRegistryServerHandle, RegistryError> {
    limits.validate(cfg.max_chunk_bytes)?;
    let listener = std::net::TcpListener::bind(&cfg.addr)
        .map_err(|e| RegistryError::Http(format!("registry/serve bind {}: {e}", cfg.addr)))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| RegistryError::Http(format!("registry/serve nonblocking: {e}")))?;
    let bound_addr = listener
        .local_addr()
        .map_err(|e| RegistryError::Http(format!("registry/serve local address: {e}")))?
        .to_string();
    let shutdown = Arc::new(Shutdown::default());
    let worker_stop = shutdown.clone();
    let join = thread::Builder::new()
        .name("genesis-registry".to_string())
        .spawn(move || run_worker(listener, cfg, limits, worker_stop))
        .map_err(|e| RegistryError::Http(format!("registry/serve spawn: {e}")))?;
    Ok(HttpRegistryServerHandle {
        shutdown,
        join: Some(join),
        bound_addr,
    })
}

fn run_worker(
    listener: std::net::TcpListener,
    cfg: HttpRegistryServerConfig,
    limits: HttpRegistryServerLimits,
    stop: Arc<Shutdown>,
) -> Result<(), RegistryError> {
    let root = cfg.root.clone();
    std::fs::create_dir_all(root.join("v1/store"))
        .map_err(|e| RegistryError::Http(format!("registry/serve mkdir: {e}")))?;
    let root = root
        .canonicalize()
        .map_err(|e| RegistryError::Http(format!("registry/serve root: {e}")))?;
    let remote = reqwest::Url::from_directory_path(&root)
        .map_err(|_| RegistryError::RemoteSpec("registry root is not absolute".to_string()))?;
    // The blocking client's runtime must be constructed/dropped outside async context.
    let client = RegistryClient::new(remote.as_str(), None)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| RegistryError::Http(format!("registry/serve runtime: {e}")))?;
    runtime.block_on(async {
        let listener = tokio::net::TcpListener::from_std(listener)
            .map_err(|e| RegistryError::Http(format!("registry/serve listener: {e}")))?;
        let uploads = Rc::new(RefCell::new(UploadState::default()));
        let mut handled = 0u64;
        loop {
            if stop.requested.load(Ordering::Acquire)
                || cfg.max_requests.is_some_and(|max| handled >= max)
            {
                break;
            }
            let accepted = transport::owned_io(
                listener.accept(),
                &stop,
                &uploads,
                &limits,
                Duration::from_secs(1),
            )
            .await;
            let stream = match accepted {
                Wait::Stopped => break,
                Wait::Deadline => continue,
                Wait::Ready(result) => {
                    result
                        .map_err(|e| RegistryError::Http(format!("registry/serve accept: {e}")))?
                        .0
                }
            };
            let admitted_request = Cell::new(false);
            let deadline = tokio::time::Instant::now() + limits.request_timeout;
            let service = service_fn(|request| {
                admitted_request.set(true);
                serve_request(
                    request,
                    &client,
                    &uploads,
                    &limits,
                    cfg.max_chunk_bytes,
                    &stop,
                    deadline,
                )
            });
            let mut http = hyper::server::conn::http1::Builder::new();
            http.keep_alive(false)
                .auto_date_header(false)
                .max_headers(64)
                .max_buf_size(16 * 1024)
                .timer(TokioTimer::new());
            let connection = http.serve_connection(TokioIo::new(stream), service);
            match transport::owned_io(connection, &stop, &uploads, &limits, limits.request_timeout)
                .await
            {
                Wait::Stopped => break,
                // Malformed/closed peers and deadlines end their owned connection.
                Wait::Ready(_) | Wait::Deadline => {}
            }
            if admitted_request.get() {
                handled = handled.saturating_add(1);
            }
        }
        Ok(())
    })
}

async fn serve_request(
    request: hyper::Request<hyper::body::Incoming>,
    client: &RegistryClient,
    uploads: &Rc<RefCell<UploadState>>,
    limits: &HttpRegistryServerLimits,
    chunk: u64,
    stop: &Shutdown,
    deadline: tokio::time::Instant,
) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>, Infallible> {
    let method = request.method().clone();
    let parsed = requests::parse_req_url(&request.uri().to_string());
    let plan = async {
        let parsed = parsed?;
        let bound = requests::body_limit(
            &method,
            &parsed.path,
            &mut uploads.borrow_mut(),
            chunk as usize,
            limits,
        )?;
        transport::check_admission(stop, deadline)?;
        let body = transport::read_body(request, bound, stop, deadline).await?;
        transport::check_admission(stop, deadline)?;
        requests::dispatch(
            &method,
            &parsed,
            body,
            client,
            &mut uploads.borrow_mut(),
            chunk,
            limits,
        )
    }
    .await;
    let (status, content_type, body) = match plan {
        Ok(response) => response,
        Err(error) => requests::error_response(error, limits.max_json_bytes),
    };
    Ok(transport::response(status, content_type, body))
}
