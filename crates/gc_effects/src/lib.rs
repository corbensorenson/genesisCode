mod error;
mod lock;
mod log;
mod policy;
mod refs;
mod runner;
mod store;

pub use crate::error::EffectsError;
pub use crate::log::{Decision, EffectLog, EffectLogEntry, LoggedResp};
pub use crate::policy::{CapsPolicy, OpPolicy};
pub use crate::refs::{RefEntry, RefsDb, SetResult};
pub use crate::runner::{RunResult, replay, replay_with_store, run};
pub use crate::store::ArtifactStore;

#[cfg(test)]
mod tests {
    use gc_coreform::TermOrdKey;
    use gc_coreform::{Term, hash_module, parse_module};
    use gc_kernel::{EvalCtx, Value, eval_module, value_hash};
    use gc_prelude::build_prelude;

    use super::*;

    fn mk_prog_for(op: &str, payload_src: &str) -> (Vec<Term>, [u8; 32]) {
        let src = format!(
            "
            (def prog
              (core/effect::perform
                '{op}
                {payload_src}
                (fn (t) (core/effect::pure t))))
            prog
        "
        );
        let forms = parse_module(&src).expect("parse module");
        let h = hash_module(&forms);
        (forms, h)
    }

    fn mk_prog() -> (Vec<Term>, [u8; 32]) {
        mk_prog_for("sys/time::now", "nil")
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

    #[test]
    fn replay_detects_tampered_op() {
        let (forms, h) = mk_prog();

        let mut ctx1 = EvalCtx::new();
        let prelude1 = build_prelude(&mut ctx1);
        let mut env1 = prelude1.env;
        let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");

        let pol = CapsPolicy::from_toml_str(r#"allow = ["sys/time::now"]"#).unwrap();
        let mut r1 = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

        r1.log.entries[0].op = "sys/time::nope".to_string();

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

    #[test]
    fn replay_detects_tampered_payload_hash() {
        let (forms, h) = mk_prog();

        let mut ctx1 = EvalCtx::new();
        let prelude1 = build_prelude(&mut ctx1);
        let mut env1 = prelude1.env;
        let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");

        let pol = CapsPolicy::from_toml_str(r#"allow = ["sys/time::now"]"#).unwrap();
        let mut r1 = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

        r1.log.entries[0].payload_h[0] ^= 0xff;

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

    #[test]
    fn replay_detects_tampered_continuation_hash() {
        let (forms, h) = mk_prog();

        let mut ctx1 = EvalCtx::new();
        let prelude1 = build_prelude(&mut ctx1);
        let mut env1 = prelude1.env;
        let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");

        let pol = CapsPolicy::from_toml_str(r#"allow = ["sys/time::now"]"#).unwrap();
        let mut r1 = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

        r1.log.entries[0].cont_h[0] ^= 0xff;

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

    #[test]
    fn replay_detects_tampered_request_hash() {
        let (forms, h) = mk_prog();

        let mut ctx1 = EvalCtx::new();
        let prelude1 = build_prelude(&mut ctx1);
        let mut env1 = prelude1.env;
        let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");

        let pol = CapsPolicy::from_toml_str(r#"allow = ["sys/time::now"]"#).unwrap();
        let mut r1 = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

        r1.log.entries[0].req_h[0] ^= 0xff;

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

    #[test]
    fn large_byte_responses_are_externalized_to_artifact_store_and_replay_loads_them() {
        let td = tempfile::tempdir().unwrap();
        let base = td.path().join("sandbox");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("in.bin"), vec![7u8; 128]).unwrap();

        let caps_path = td.path().join("caps.toml");
        std::fs::write(
            &caps_path,
            r#"
allow = ["io/fs::read"]

[log]
inline_max_bytes = 8
store_dir = "./.genesis/store"

[op."io/fs::read"]
base_dir = "./sandbox"
"#,
        )
        .unwrap();
        let pol = CapsPolicy::load(&caps_path).unwrap();

        let src = r#"
            (def prog
              (core/effect::perform
                'io/fs::read
                {:path "in.bin"}
                (fn (b) (core/effect::pure b))))
            prog
        "#;
        let forms = parse_module(src).expect("parse module");
        let h = hash_module(&forms);

        let mut ctx1 = EvalCtx::new();
        let prelude1 = build_prelude(&mut ctx1);
        let mut env1 = prelude1.env;
        let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");

        let r1 = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

        assert!(matches!(
            r1.log.entries[0].resp,
            LoggedResp::OkBytesArtifact { .. }
        ));

        let store = ArtifactStore::open(pol.store_dir().unwrap()).unwrap();
        let mut ctx2 = EvalCtx::new();
        let prelude2 = build_prelude(&mut ctx2);
        let mut env2 = prelude2.env;
        let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");

        let v2 = replay_with_store(&mut ctx2, prog2, &r1.log, Some(&store)).expect("replay");
        assert_eq!(value_hash(&r1.value), value_hash(&v2));

        // Without a store, replay must fail deterministically.
        let mut ctx3 = EvalCtx::new();
        let prelude3 = build_prelude(&mut ctx3);
        let mut env3 = prelude3.env;
        let prog3 = eval_module(&mut ctx3, &mut env3, &forms).expect("eval3");
        let err = replay(&mut ctx3, prog3, &r1.log).unwrap_err();
        assert!(
            matches!(err, EffectsError::ReplayMismatch(_)),
            "expected replay mismatch, got {err}"
        );
    }

    #[test]
    fn cap_term_omits_base_dir_paths_for_deterministic_logs() {
        let td = tempfile::tempdir().unwrap();
        let base = td.path().join("sandbox");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("in.bin"), vec![1u8, 2, 3]).unwrap();

        let caps_path = td.path().join("caps.toml");
        std::fs::write(
            &caps_path,
            r#"
allow = ["io/fs::read"]

[op."io/fs::read"]
base_dir = "./sandbox"
"#,
        )
        .unwrap();
        let pol = CapsPolicy::load(&caps_path).unwrap();

        let src = r#"
            (def prog
              (core/effect::perform
                'io/fs::read
                {:path "in.bin"}
                (fn (b) (core/effect::pure b))))
            prog
        "#;
        let forms = parse_module(src).expect("parse module");
        let h = hash_module(&forms);

        let mut ctx = EvalCtx::new();
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");

        let r = run(&mut ctx, &pol, prog, h, "gc_effects-test".to_string()).expect("run");
        let Term::Map(m) = &r.log.entries[0].cap else {
            panic!("expected cap map");
        };
        assert!(
            !m.contains_key(&TermOrdKey(Term::symbol(":base-dir"))),
            "cap term must not include :base-dir"
        );
    }

    #[test]
    fn gfx_frame_tick_is_supported_and_replayable() {
        let (forms, h) = mk_prog_for("gfx/time::frame-tick", "{:surface \"main\"}");

        let mut ctx1 = EvalCtx::new();
        let prelude1 = build_prelude(&mut ctx1);
        let mut env1 = prelude1.env;
        let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");

        let pol = CapsPolicy::from_toml_str(r#"allow = ["gfx/time::frame-tick"]"#).unwrap();
        let r1 = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

        match &r1.value {
            Value::Data(Term::Map(m)) => {
                assert!(matches!(
                    m.get(&TermOrdKey(Term::symbol(":time-ms"))),
                    Some(Term::Int(_))
                ));
            }
            other => panic!("expected frame-tick map, got {}", other.debug_repr()),
        }

        let mut ctx2 = EvalCtx::new();
        let prelude2 = build_prelude(&mut ctx2);
        let mut env2 = prelude2.env;
        let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");
        let v2 = replay(&mut ctx2, prog2, &r1.log).expect("replay");
        assert_eq!(value_hash(&r1.value), value_hash(&v2));
    }

    #[test]
    fn known_editor_capability_returns_not_supported_error() {
        let (forms, h) = mk_prog_for("editor/clipboard::get", "{}");

        let mut ctx = EvalCtx::new();
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");

        let pol = CapsPolicy::from_toml_str(r#"allow = ["editor/clipboard::get"]"#).unwrap();
        let r = run(&mut ctx, &pol, prog, h, "gc_effects-test".to_string()).expect("run");

        match r.value {
            Value::Sealed { token, payload } => {
                assert_eq!(token, ctx.protocol.expect("protocol").error);
                let Value::Data(Term::Map(m)) = payload.as_ref() else {
                    panic!("expected error payload map");
                };
                assert_eq!(
                    m.get(&TermOrdKey(Term::symbol(":error/code"))),
                    Some(&Term::Str("core/caps/not-supported".to_string()))
                );
            }
            other => panic!("expected sealed error, got {}", other.debug_repr()),
        }
    }
}
