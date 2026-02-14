mod error;
mod log;
mod policy;
mod runner;

pub use crate::error::EffectsError;
pub use crate::log::{Decision, EffectLog, EffectLogEntry, LoggedResp};
pub use crate::policy::{CapsPolicy, OpPolicy};
pub use crate::runner::{RunResult, replay, run};

#[cfg(test)]
mod tests {
    use gc_coreform::{Term, hash_module, parse_module};
    use gc_kernel::{EvalCtx, Value, eval_module, value_hash};
    use gc_prelude::build_prelude;

    use super::*;

    fn mk_prog() -> (Vec<Term>, [u8; 32]) {
        let src = r#"
            (def prog
              (core/effect::perform
                'sys/time::now
                nil
                (fn (t) (core/effect::pure t))))
            prog
        "#;
        let forms = parse_module(src).expect("parse module");
        let h = hash_module(&forms);
        (forms, h)
    }

    #[test]
    fn deny_by_default_produces_sealed_error_and_logs() {
        let (forms, h) = mk_prog();

        let mut ctx = EvalCtx::new();
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");

        let pol = CapsPolicy::empty();
        let r = run(&mut ctx, &pol, prog, h, "gc_effects-test".to_string()).expect("run");

        // Expect final value is a sealed ERROR.
        match r.value {
            Value::Sealed { token, .. } => {
                assert_eq!(token, ctx.protocol.unwrap().error);
            }
            _ => panic!("expected sealed error, got {}", r.value.debug_repr()),
        }

        assert_eq!(r.log.entries.len(), 1);
        assert_eq!(r.log.entries[0].decision, Decision::Deny);
        assert!(matches!(r.log.entries[0].resp, LoggedResp::Error(_)));
    }

    #[test]
    fn allow_and_replay_roundtrip() {
        let (forms, h) = mk_prog();

        let mut ctx1 = EvalCtx::new();
        let prelude1 = build_prelude(&mut ctx1);
        let mut env1 = prelude1.env;
        let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");

        let pol = CapsPolicy::from_toml_str(r#"allow = ["sys/time::now"]"#).unwrap();
        let r1 = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

        let v1_h = value_hash(&r1.value);

        // Parse log back from term and replay with fresh ctx.
        let log_term = r1.log.to_term();
        let log2 = EffectLog::from_term(&log_term).expect("parse log");

        let mut ctx2 = EvalCtx::new();
        let prelude2 = build_prelude(&mut ctx2);
        let mut env2 = prelude2.env;
        let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");

        let v2 = replay(&mut ctx2, prog2, &log2).expect("replay");
        let v2_h = value_hash(&v2);
        assert_eq!(v1_h, v2_h);
    }

    #[test]
    fn replay_detects_tampered_response() {
        let (forms, h) = mk_prog();

        let mut ctx1 = EvalCtx::new();
        let prelude1 = build_prelude(&mut ctx1);
        let mut env1 = prelude1.env;
        let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");

        let pol = CapsPolicy::from_toml_str(r#"allow = ["sys/time::now"]"#).unwrap();
        let mut r1 = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

        // Tamper.
        if let LoggedResp::Ok(t) = &mut r1.log.entries[0].resp {
            *t = Term::Nil;
        }

        let mut ctx2 = EvalCtx::new();
        let prelude2 = build_prelude(&mut ctx2);
        let mut env2 = prelude2.env;
        let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");

        let err = replay(&mut ctx2, prog2, &r1.log).unwrap_err();
        assert!(
            matches!(err, EffectsError::ReplayMismatch(_)),
            "expected replay mismatch, got {err}"
        );
    }
}
