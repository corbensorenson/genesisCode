#![cfg(feature = "device-bridge")]

use std::path::PathBuf;

use gc_coreform::{Term, TermOrdKey, hash_module, parse_module};
use gc_effects::{CapsPolicy, replay, run};
use gc_kernel::{EvalCtx, Value, eval_module, value_hash};
use gc_prelude::build_prelude;

fn parse_and_eval(ctx: &mut EvalCtx, src: &str) -> (Value, [u8; 32]) {
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let prelude = build_prelude(ctx);
    let mut env = prelude.env;
    let prog = eval_module(ctx, &mut env, &forms).expect("eval module");
    (prog, h)
}

fn toml_escape(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('"', "\\\"")
}

#[test]
fn device_bridge_submit_is_replay_deterministic() {
    let bridge_path = locate_runtime_bench_binary();
    let base_dir = bridge_path
        .parent()
        .expect("bridge executable parent directory");
    let bridge_name = bridge_path
        .file_name()
        .and_then(|s| s.to_str())
        .expect("bridge executable name");

    let policy = CapsPolicy::from_toml_str(&format!(
        r#"
allow = ["gpu/compute::submit"]

[op."gpu/compute::submit"]
base_dir = "{}"
bridge_cmd = "{}"
bridge_args = ["--gpu-compute-bridge"]
max_bytes = 65536
"#,
        toml_escape(&base_dir.display().to_string()),
        toml_escape(bridge_name),
    ))
    .expect("parse device bridge policy");

    let src = r#"
(def prog
  (core/effect::perform
    'gpu/compute::submit
    {:graph {:passes [{:dispatch {:x 128 :y 1 :z 1}
                       :kernel "bench/matmul"
                       :bindings [{:buffer "a"} {:buffer "b"} {:buffer "out"}]}]}}
    (fn (x) (core/effect::pure x))))
prog
"#;

    let mut ctx_run = EvalCtx::new();
    let (program_run, hash_run) = parse_and_eval(&mut ctx_run, src);
    let run_out = run(
        &mut ctx_run,
        &policy,
        program_run,
        hash_run,
        "runtime-bench-device-bridge-test".to_string(),
    )
    .expect("run with device bridge");

    let Value::Data(Term::Map(run_map)) = &run_out.value else {
        panic!("device bridge result must be data map");
    };
    assert_eq!(
        run_map.get(&TermOrdKey(Term::symbol(":backend"))),
        Some(&Term::Str("device-runtime".to_string()))
    );
    assert_eq!(
        run_map.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(&Term::Bool(true))
    );

    let mut ctx_replay = EvalCtx::new();
    let (program_replay, _) = parse_and_eval(&mut ctx_replay, src);
    let replay_out = replay(&mut ctx_replay, program_replay, &run_out.log).expect("replay");
    assert_eq!(value_hash(&run_out.value), value_hash(&replay_out));
}

fn locate_runtime_bench_binary() -> PathBuf {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_gc_runtime_bench") {
        let p = PathBuf::from(path);
        if p.is_file() {
            return p;
        }
    }

    let current = std::env::current_exe().expect("resolve current test executable path");
    let deps_dir = current
        .parent()
        .expect("test executable must have a parent directory");
    let target_dir = deps_dir
        .parent()
        .expect("deps directory must have a parent target directory");
    let exe_name = if cfg!(windows) {
        "gc_runtime_bench.exe"
    } else {
        "gc_runtime_bench"
    };
    let candidate = target_dir.join(exe_name);
    assert!(
        candidate.is_file(),
        "runtime bench binary not found at {}",
        candidate.display()
    );
    candidate
}
