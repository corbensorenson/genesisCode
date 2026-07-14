use std::time::Duration;

use serde_json::{Value, json};

use super::json_canonical_string;

pub(super) const SESSION_AUDIT_V01: &str = "genesis/agent-session-audit-v0.1";

#[derive(Clone, Debug)]
pub(super) struct SessionResourceLimits {
    pub(super) max_wall: Duration,
    pub(super) max_cpu: Duration,
    pub(super) max_steps: u64,
    pub(super) max_heap_bytes: u64,
    pub(super) max_output_bytes: usize,
    pub(super) max_effects: u64,
    pub(super) max_processes: u64,
    pub(super) max_disk_bytes: u64,
    pub(super) max_drain_requests: usize,
    pub(super) drain_timeout: Duration,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct SessionResourceOptions {
    pub(super) max_wall_ms: u64,
    pub(super) max_cpu_ms: u64,
    pub(super) max_steps: u64,
    pub(super) max_heap_bytes: u64,
    pub(super) max_output_bytes: usize,
    pub(super) max_effects: u64,
    pub(super) max_processes: u64,
    pub(super) max_disk_bytes: u64,
    pub(super) max_drain_requests: usize,
    pub(super) drain_timeout_ms: u64,
}

impl SessionResourceLimits {
    pub(super) fn from_options(options: SessionResourceOptions) -> Result<Self, &'static str> {
        if !(1..=86_400_000).contains(&options.max_wall_ms) {
            return Err("max_wall_ms must be in 1..=86400000");
        }
        if !(1..=86_400_000).contains(&options.max_cpu_ms) {
            return Err("max_cpu_ms must be in 1..=86400000");
        }
        if options.max_steps == 0 {
            return Err("max_steps must be positive");
        }
        if !(64 * 1024 * 1024..=64 * 1024 * 1024 * 1024).contains(&options.max_heap_bytes) {
            return Err("max_heap_bytes must be in 67108864..=68719476736");
        }
        if !(1024..=64 * 1024 * 1024).contains(&options.max_output_bytes) {
            return Err("max_output_bytes must be in 1024..=67108864");
        }
        if options.max_effects == 0 {
            return Err("max_effects must be positive");
        }
        if !(1..=64).contains(&options.max_processes) {
            return Err("max_processes must be in 1..=64");
        }
        if !(1024 * 1024..=1024 * 1024 * 1024 * 1024).contains(&options.max_disk_bytes) {
            return Err("max_disk_bytes must be in 1048576..=1099511627776");
        }
        if options.max_drain_requests > 4096 {
            return Err("max_drain_requests must be in 0..=4096");
        }
        if !(1..=3_600_000).contains(&options.drain_timeout_ms) {
            return Err("drain_timeout_ms must be in 1..=3600000");
        }
        Ok(Self {
            max_wall: Duration::from_millis(options.max_wall_ms),
            max_cpu: Duration::from_millis(options.max_cpu_ms),
            max_steps: options.max_steps,
            max_heap_bytes: options.max_heap_bytes,
            max_output_bytes: options.max_output_bytes,
            max_effects: options.max_effects,
            max_processes: options.max_processes,
            max_disk_bytes: options.max_disk_bytes,
            max_drain_requests: options.max_drain_requests,
            drain_timeout: Duration::from_millis(options.drain_timeout_ms),
        })
    }

    pub(super) fn as_json(&self) -> Value {
        json!({
            "max_wall_ms": self.max_wall.as_millis(),
            "max_cpu_ms": self.max_cpu.as_millis(),
            "max_steps": self.max_steps,
            "max_heap_bytes": self.max_heap_bytes,
            "max_output_bytes": self.max_output_bytes,
            "max_effects": self.max_effects,
            "max_processes": self.max_processes,
            "max_disk_bytes": self.max_disk_bytes,
            "max_drain_requests": self.max_drain_requests,
            "drain_timeout_ms": self.drain_timeout.as_millis(),
        })
    }

    pub(super) fn identity(&self) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"GCv0.2\0agent-session-resource-limits-v0.1\0");
        hasher.update(json_canonical_string(&self.as_json()).as_bytes());
        hasher.finalize().to_hex().to_string()
    }
}

#[derive(Clone, Debug)]
pub(super) struct SessionAudit {
    pub(super) worker_profile: &'static str,
    pub(super) limits_identity: String,
    pub(super) wall_ms: u64,
    pub(super) cpu_ms: Option<u64>,
    pub(super) output_bytes: u64,
    pub(super) disk_delta_bytes: i64,
    pub(super) effect_ops: Option<u64>,
    pub(super) peak_heap_bytes: Option<u64>,
    pub(super) peak_processes: Option<u64>,
    pub(super) termination: &'static str,
    pub(super) exceeded: Option<&'static str>,
}

impl SessionAudit {
    pub(super) fn not_started(limits: &SessionResourceLimits, termination: &'static str) -> Self {
        Self {
            worker_profile: "not-started-v0.1",
            limits_identity: limits.identity(),
            wall_ms: 0,
            cpu_ms: Some(0),
            output_bytes: 0,
            disk_delta_bytes: 0,
            effect_ops: Some(0),
            peak_heap_bytes: Some(0),
            peak_processes: Some(0),
            termination,
            exceeded: None,
        }
    }

    pub(super) fn as_json(&self) -> Value {
        let enforcement = if self.worker_profile == "native-isolated-v0.1" {
            json!({
                "wall": "process-tree-deadline",
                "cpu": "process-tree-cpu",
                "steps": "kernel-step-limit",
                "heap": "process-tree-rss-plus-kernel-shapes",
                "output": "combined-draining-pipes",
                "effects": "runner-effect-ops",
                "processes": "process-tree-count",
                "disk": "file-size-plus-workspace-growth",
            })
        } else if self.worker_profile == "wasi-inline-v0.1" {
            json!({
                "wall": "cooperative-inline",
                "cpu": "unavailable-inline",
                "steps": "kernel-step-limit",
                "heap": "kernel-shape-limits",
                "output": "bounded-response",
                "effects": "runner-effect-ops",
                "processes": "unavailable",
                "disk": "post-execution-workspace-growth",
            })
        } else {
            json!({
                "wall": "not-started",
                "cpu": "not-started",
                "steps": "not-started",
                "heap": "not-started",
                "output": "not-started",
                "effects": "not-started",
                "processes": "not-started",
                "disk": "not-started",
            })
        };
        json!({
            "kind": SESSION_AUDIT_V01,
            "worker_profile": self.worker_profile,
            "limits_identity": self.limits_identity,
            "observed": {
                "wall_ms": self.wall_ms,
                "cpu_ms": self.cpu_ms,
                "output_bytes": self.output_bytes,
                "disk_delta_bytes": self.disk_delta_bytes,
                "effect_ops": self.effect_ops,
                "peak_heap_bytes": self.peak_heap_bytes,
                "peak_processes": self.peak_processes,
            },
            "enforcement": enforcement,
            "termination": self.termination,
            "exceeded": self.exceeded,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> SessionResourceOptions {
        SessionResourceOptions {
            max_wall_ms: 10_000,
            max_cpu_ms: 5_000,
            max_steps: 50_000_000,
            max_heap_bytes: 512 * 1024 * 1024,
            max_output_bytes: 1024 * 1024,
            max_effects: 1024,
            max_processes: 1,
            max_disk_bytes: 64 * 1024 * 1024,
            max_drain_requests: 8,
            drain_timeout_ms: 2_000,
        }
    }

    #[test]
    fn resource_identity_is_stable_and_every_dimension_is_finite() {
        let limits = SessionResourceLimits::from_options(options()).expect("valid limits");
        assert_eq!(limits.identity().len(), 64);
        assert_eq!(limits.as_json()["max_processes"], 1);
        assert_eq!(limits.as_json()["max_drain_requests"], 8);
    }

    #[test]
    fn unsafe_or_unenforceable_limits_fail_closed() {
        let mut invalid = options();
        invalid.max_processes = 65;
        assert!(SessionResourceLimits::from_options(invalid).is_err());
        invalid = options();
        invalid.max_heap_bytes = 1;
        assert!(SessionResourceLimits::from_options(invalid).is_err());
        invalid = options();
        invalid.max_drain_requests = 4097;
        assert!(SessionResourceLimits::from_options(invalid).is_err());
    }

    #[test]
    fn complete_native_audit_fits_the_minimum_transport_frame_budget() {
        let limits = SessionResourceLimits::from_options(options()).expect("valid limits");
        let audit = SessionAudit {
            worker_profile: "native-isolated-v0.1",
            limits_identity: limits.identity(),
            wall_ms: u64::MAX,
            cpu_ms: Some(u64::MAX),
            output_bytes: u64::MAX,
            disk_delta_bytes: i64::MIN,
            effect_ops: Some(u64::MAX),
            peak_heap_bytes: Some(u64::MAX),
            peak_processes: Some(u64::MAX),
            termination: "resource-killed-and-reaped",
            exceeded: Some("processes"),
        };
        let fallback = json!({
            "jsonrpc": "2.0",
            "id": "request",
            "error": {
                "code": -32003,
                "message": "output frame exceeds configured limit",
                "data": {"audit": audit.as_json()},
            }
        });
        let bytes = json_canonical_string(&fallback).len() + 1;
        assert!(bytes <= 1024, "audited fallback requires {bytes} bytes");
    }
}
