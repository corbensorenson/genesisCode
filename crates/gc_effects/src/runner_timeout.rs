use crate::EffectsError;

type TimeoutJob = Box<dyn FnOnce() + Send + 'static>;

struct TimeoutPool {
    tx: std::sync::mpsc::SyncSender<TimeoutJob>,
}

#[derive(Clone)]
pub(crate) struct TimeoutCancelToken {
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl TimeoutCancelToken {
    fn new() -> Self {
        Self {
            cancelled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Release);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Acquire)
    }
}

impl TimeoutPool {
    fn worker_count() -> usize {
        let env_n = std::env::var("GENESIS_TIMEOUT_WORKERS")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(4);
        env_n.clamp(1, 32)
    }

    fn new() -> Result<Self, EffectsError> {
        use std::sync::{Arc, Mutex, mpsc};
        let queue_n = std::env::var("GENESIS_TIMEOUT_QUEUE")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(256)
            .clamp(1, 4096);
        let (tx, rx) = mpsc::sync_channel::<TimeoutJob>(queue_n);
        let rx = Arc::new(Mutex::new(rx));

        let mut started = 0usize;
        for i in 0..Self::worker_count() {
            let rx2 = Arc::clone(&rx);
            let name = format!("gc-timeout-{i}");
            let spawned = std::thread::Builder::new().name(name).spawn(move || {
                loop {
                    let msg = {
                        let guard = rx2.lock();
                        match guard {
                            Ok(g) => g.recv(),
                            Err(_) => return,
                        }
                    };
                    match msg {
                        Ok(job) => job(),
                        Err(_) => return,
                    }
                }
            });
            if spawned.is_ok() {
                started = started.saturating_add(1);
            }
        }
        if started == 0 {
            return Err(EffectsError::Log(
                "timeout pool failed to start worker threads".to_string(),
            ));
        }
        Ok(Self { tx })
    }

    fn submit(&self, job: TimeoutJob) -> Result<(), TimeoutJob> {
        match self.tx.try_send(job) {
            Ok(()) => Ok(()),
            Err(std::sync::mpsc::TrySendError::Full(job)) => Err(job),
            Err(std::sync::mpsc::TrySendError::Disconnected(job)) => Err(job),
        }
    }
}

fn timeout_pool() -> Option<&'static TimeoutPool> {
    static POOL: std::sync::OnceLock<TimeoutPool> = std::sync::OnceLock::new();
    if let Some(p) = POOL.get() {
        return Some(p);
    }
    let built = TimeoutPool::new().ok()?;
    if POOL.set(built).is_ok() {
        return POOL.get();
    }
    POOL.get()
}

pub(crate) fn with_timeout<T, F>(timeout_ms: u64, f: F) -> Result<Option<T>, EffectsError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, EffectsError> + Send + 'static,
{
    with_timeout_cancellable(timeout_ms, move |_| f())
}

pub(crate) fn with_timeout_cancellable<T, F>(
    timeout_ms: u64,
    f: F,
) -> Result<Option<T>, EffectsError>
where
    T: Send + 'static,
    F: FnOnce(TimeoutCancelToken) -> Result<T, EffectsError> + Send + 'static,
{
    use std::sync::mpsc;
    let cancel = TimeoutCancelToken::new();
    let cancel_job = cancel.clone();
    let (tx, rx) = mpsc::channel();
    let job: TimeoutJob = Box::new(move || {
        let r = f(cancel_job);
        let _ = tx.send(r);
    });
    let Some(pool) = timeout_pool() else {
        return Err(EffectsError::Log(
            "timeout worker pool unavailable".to_string(),
        ));
    };
    if pool.submit(job).is_err() {
        return Err(EffectsError::Log(
            "timeout worker pool is saturated".to_string(),
        ));
    }
    match rx.recv_timeout(std::time::Duration::from_millis(timeout_ms)) {
        Ok(r) => r.map(Some),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            cancel.cancel();
            Ok(None)
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(EffectsError::Log(
            "capability thread disconnected".to_string(),
        )),
    }
}

#[cfg(test)]
mod timeout_tests {
    use super::{with_timeout, with_timeout_cancellable};
    use crate::EffectsError;

    #[test]
    fn with_timeout_returns_some_for_fast_jobs() {
        let out = with_timeout(100, || -> Result<u64, EffectsError> { Ok(7) }).unwrap();
        assert_eq!(out, Some(7));
    }

    #[test]
    fn with_timeout_returns_none_for_slow_jobs() {
        let out = with_timeout(1, || -> Result<u64, EffectsError> {
            std::thread::sleep(std::time::Duration::from_millis(25));
            Ok(7)
        })
        .unwrap();
        assert_eq!(out, None);
    }

    #[test]
    fn with_timeout_cancellable_cooperative_jobs_drain_under_load() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let running = Arc::new(AtomicUsize::new(0));

        for _ in 0..64 {
            let running2 = Arc::clone(&running);
            let out = with_timeout_cancellable(1, move |tok| -> Result<u64, EffectsError> {
                running2.fetch_add(1, Ordering::SeqCst);
                while !tok.is_cancelled() {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                running2.fetch_sub(1, Ordering::SeqCst);
                Ok(0)
            })
            .unwrap();
            assert_eq!(out, None);
        }

        for _ in 0..200 {
            if running.load(Ordering::SeqCst) == 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(running.load(Ordering::SeqCst), 0);
    }
}
