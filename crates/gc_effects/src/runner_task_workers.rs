use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use super::*;

struct ActiveTaskWorker(Arc<AtomicUsize>);

impl ActiveTaskWorker {
    fn enter(active: Arc<AtomicUsize>) -> Self {
        active.fetch_add(1, Ordering::SeqCst);
        Self(active)
    }
}

impl Drop for ActiveTaskWorker {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
std::thread_local! {
    static INJECT_TASK_JOIN_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl TaskRuntime {
    pub(super) fn wait_for_completion(&mut self, task_id: &str) -> Result<TaskCompletion, String> {
        if let Some(c) = self.completed.remove(task_id) {
            return Ok(c);
        }
        self.pool.wait_for_completion(task_id, &mut self.completed)
    }

    pub(crate) fn shutdown(&mut self) -> Result<(), String> {
        for task in self.tasks.values() {
            if let Some(cancel_flag) = &task.cancel_flag {
                cancel_flag.store(true, Ordering::Release);
            }
        }
        self.pool.shutdown()
    }
}

impl TaskWorkerPool {
    pub(super) fn ensure_started(&mut self, worker_count: usize) {
        if self.tx.is_some() || worker_count == 0 {
            return;
        }
        let (tx, rx) = mpsc::channel::<TaskJob>();
        let (done_tx, done_rx) = mpsc::channel::<TaskCompletion>();
        let (ready_tx, ready_rx) = mpsc::channel::<()>();
        let shared_rx = Arc::new(Mutex::new(rx));
        let mut started_workers = 0usize;
        for idx in 0..worker_count {
            let shared_rx = Arc::clone(&shared_rx);
            let done_tx = done_tx.clone();
            let ready_tx = ready_tx.clone();
            let active = Arc::clone(&self.active);
            let worker_result = thread::Builder::new()
                .name(format!("genesis-task-{idx}"))
                .spawn(move || worker_loop(shared_rx, done_tx, ready_tx, active));
            match worker_result {
                Ok(worker) => {
                    self.workers.push(worker);
                    started_workers = started_workers.saturating_add(1);
                }
                Err(_) => break,
            }
        }
        if started_workers == 0 {
            return;
        }
        for _ in 0..started_workers {
            let _ = ready_rx.recv_timeout(Duration::from_secs(1));
        }
        self.tx = Some(tx);
        self.rx = Some(done_rx);
    }

    pub(super) fn shutdown(&mut self) -> Result<(), String> {
        self.tx.take();
        let mut failures = Vec::new();
        let mut joined = 0_usize;
        for (index, worker) in self.workers.drain(..).enumerate() {
            match worker.join() {
                Ok(()) => joined = joined.saturating_add(1),
                Err(_) => failures.push(format!("task worker {index} panicked during join")),
            }
        }
        self.rx.take();
        let active = self.active.load(Ordering::SeqCst);
        if active != 0 {
            failures.push(format!(
                "{active} task worker(s) remained active after deterministic join"
            ));
        }
        #[cfg(test)]
        if joined > 0 && INJECT_TASK_JOIN_FAILURE.get() {
            failures.push("injected task worker join failure".to_string());
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }

    pub(super) fn dispatch(&self, job: TaskJob) -> Result<(), String> {
        match &self.tx {
            Some(tx) => tx
                .send(job)
                .map_err(|e| format!("task dispatch failed: {e}")),
            None => Err("task worker pool not initialized".to_string()),
        }
    }

    pub(super) fn wait_for_completion(
        &mut self,
        task_id: &str,
        completed: &mut BTreeMap<String, TaskCompletion>,
    ) -> Result<TaskCompletion, String> {
        if let Some(c) = completed.remove(task_id) {
            return Ok(c);
        }
        let rx = self
            .rx
            .as_ref()
            .ok_or_else(|| "task worker completion channel not initialized".to_string())?;
        loop {
            let completion = rx
                .recv()
                .map_err(|e| format!("task completion channel closed: {e}"))?;
            if completion.task_id == task_id {
                return Ok(completion);
            }
            completed.insert(completion.task_id.clone(), completion);
        }
    }
}

fn worker_loop(
    shared_rx: Arc<Mutex<mpsc::Receiver<TaskJob>>>,
    done_tx: mpsc::Sender<TaskCompletion>,
    ready_tx: mpsc::Sender<()>,
    active: Arc<AtomicUsize>,
) {
    let _active = ActiveTaskWorker::enter(active);
    let _ = ready_tx.send(());
    loop {
        let recv = match shared_rx.lock() {
            Ok(rx) => rx.recv(),
            Err(poisoned) => poisoned.into_inner().recv(),
        };
        let job = match recv {
            Ok(job) => job,
            Err(_) => break,
        };
        let outcome = execute_task_payload(job.payload, &job.cancel_flag, job.policy.as_ref());
        let _ = done_tx.send(TaskCompletion {
            task_id: job.task_id,
            outcome,
        });
    }
}

#[cfg(test)]
pub(crate) fn with_task_join_failure_for_tests<T>(operation: impl FnOnce() -> T) -> T {
    struct ResetFault(bool);

    impl Drop for ResetFault {
        fn drop(&mut self) {
            INJECT_TASK_JOIN_FAILURE.set(self.0);
        }
    }

    let previous = INJECT_TASK_JOIN_FAILURE.replace(true);
    let _reset = ResetFault(previous);
    operation()
}
