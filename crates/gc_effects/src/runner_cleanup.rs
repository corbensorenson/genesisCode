use super::RunResult;
use crate::error::EffectsError;
use crate::runner_host_bridge::BridgeError;

pub(super) fn finalize_run_with_runtime_cleanup(
    outcome: Result<RunResult, EffectsError>,
    task_cleanup: Result<(), String>,
    bridge_cleanup: Result<(), BridgeError>,
) -> Result<RunResult, EffectsError> {
    let mut failures = Vec::new();
    if let Err(error) = task_cleanup {
        failures.push(("task-worker", error));
    }
    if let Err(error) = bridge_cleanup {
        failures.push(("host-bridge", format!("{}: {}", error.code, error.message)));
    }
    if failures.is_empty() {
        return outcome;
    }
    let subsystem = if failures.len() == 1 {
        failures[0].0.to_string()
    } else {
        "runtime".to_string()
    };
    let reason = if failures.len() == 1 {
        failures
            .pop()
            .map(|(_, failure)| failure)
            .unwrap_or_default()
    } else {
        failures
            .into_iter()
            .map(|(owner, failure)| format!("{owner}: {failure}"))
            .collect::<Vec<_>>()
            .join("; ")
    };
    Err(EffectsError::Cleanup {
        subsystem,
        reason,
        prior_error: outcome.err().map(|error| error.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_cleanup_failure_preserves_the_prior_run_error() {
        let outcome = Err(EffectsError::BadEffectSeal);
        let bridge_cleanup = Err(BridgeError {
            code: "net/bridge-reap".to_string(),
            message: "worker reap failed".to_string(),
        });
        let error = match finalize_run_with_runtime_cleanup(outcome, Ok(()), bridge_cleanup) {
            Ok(_) => panic!("cleanup failure must cross the run boundary"),
            Err(error) => error,
        };
        let EffectsError::Cleanup {
            subsystem,
            reason,
            prior_error,
        } = error
        else {
            panic!("expected typed cleanup failure");
        };
        assert_eq!(subsystem, "host-bridge");
        assert_eq!(reason, "net/bridge-reap: worker reap failed");
        assert_eq!(
            prior_error.as_deref(),
            Some("effect request is not sealed with the EFFECT protocol token")
        );
    }

    #[test]
    fn task_and_bridge_cleanup_failures_are_aggregated_in_owner_order() {
        let bridge_cleanup = Err(BridgeError {
            code: "net/bridge-reap".to_string(),
            message: "worker reap failed".to_string(),
        });
        let error = match finalize_run_with_runtime_cleanup(
            Err(EffectsError::BadEffectSeal),
            Err("task worker join failed".to_string()),
            bridge_cleanup,
        ) {
            Ok(_) => panic!("cleanup failures must cross the run boundary"),
            Err(error) => error,
        };
        let EffectsError::Cleanup {
            subsystem,
            reason,
            prior_error,
        } = error
        else {
            panic!("expected typed cleanup failure");
        };
        assert_eq!(subsystem, "runtime");
        assert_eq!(
            reason,
            "task-worker: task worker join failed; host-bridge: net/bridge-reap: worker reap failed"
        );
        assert_eq!(
            prior_error.as_deref(),
            Some("effect request is not sealed with the EFFECT protocol token")
        );
    }
}
