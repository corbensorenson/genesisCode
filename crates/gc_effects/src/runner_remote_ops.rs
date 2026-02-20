use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

include!("runner_remote_ops/policy_auth.rs");
include!("runner_remote_ops/sync_closure_parallel.rs");
include!("runner_remote_ops/sync_capabilities.rs");
include!("runner_remote_ops/gpk.rs");
