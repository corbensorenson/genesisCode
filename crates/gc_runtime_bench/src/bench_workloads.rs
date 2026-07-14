use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use gc_coreform::{Term, canonicalize_module, parse_module};
use gc_kernel::{Apply, Env, EvalCtx, Value, compile_module, eval_compiled_module};
use gc_prelude::build_prelude;

use crate::config::BenchConfig;
use crate::measure::best_of;
use crate::report::WorkloadMetrics;

pub fn run(cfg: &BenchConfig) -> Result<WorkloadMetrics> {
    let fib_source = workload_source(cfg, "PB-1", || fib_src(cfg.workload_sizes.fib_n))?;
    let vector_source = workload_source(cfg, "PB-4", || vec_build_src(cfg.workload_sizes.vec_len))?;
    let map_source = workload_source(cfg, "PB-5", || map_build_src(cfg.workload_sizes.map_len))?;
    Ok(WorkloadMetrics {
        fib_ms: measure_compiled_workload(cfg, "fib", &fib_source)?,
        vec_build_ms: measure_compiled_workload(cfg, "vec-build", &vector_source)?,
        map_build_ms: measure_compiled_workload(cfg, "map-build", &map_source)?,
        str_concat_ms: measure_compiled_workload(
            cfg,
            "str-concat",
            &str_concat_src(cfg.workload_sizes.str_concat_count),
        )?,
        selfhost_parse_ms: measure_selfhost_parse(cfg)?,
        dispatch_ms: measure_compiled_workload(
            cfg,
            "dispatch",
            &dispatch_src(cfg.workload_sizes.dispatch_count),
        )?,
    })
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoadmapRawSample {
    kind: &'static str,
    version: &'static str,
    workload_id: String,
    duration_ns: u128,
    expected_descriptor_sha256: &'static str,
    semantic_check: &'static str,
    unit: &'static str,
}

pub fn run_roadmap_sample(cfg: &BenchConfig, workload_id: &str) -> Result<RoadmapRawSample> {
    if cfg.workload_profile != "roadmap" {
        bail!("--roadmap-sample requires GENESIS_RUNTIME_WORKLOAD_PROFILE=roadmap");
    }
    let (duration_ns, expected_descriptor_sha256) = match workload_id {
        "PB-1" => (
            measure_one_compiled_roadmap("PB-1", "fib", ExpectedOutcome::Integer(75_025))?,
            "35f740ed69c7a1c89eff3edef9d7c3d185c53152a90a341ae202fe92a08b3731",
        ),
        "PB-4" => (
            measure_one_compiled_roadmap(
                "PB-4",
                "vec-build",
                ExpectedOutcome::IntegerRangeVector(1_000_000),
            )?,
            "8f8d5c29344d6c6ef971708f11583ba7b9a9cb9ea76681b82f6ba32650eb0ef3",
        ),
        "PB-5" => (
            measure_one_compiled_roadmap(
                "PB-5",
                "map-build",
                ExpectedOutcome::IntegerIdentityMap(100_000),
            )?,
            "b055c3f4bb2cd2829c437cd1bd406503013fe78274d26ef637d9dfe02dd74b0d",
        ),
        "PB-7" => (
            measure_one_selfhost_parse(cfg)?,
            "dc0f62caf81b2cd4969a7d0614e8b83cbeafdd2fd11e418b95f571d0ffa94429",
        ),
        _ => bail!("roadmap sample runner is unavailable for {workload_id}"),
    };
    Ok(RoadmapRawSample {
        kind: "genesis/roadmap-workload-raw-sample-v0.1",
        version: "0.1",
        workload_id: workload_id.to_string(),
        duration_ns,
        expected_descriptor_sha256,
        semantic_check: "passed",
        unit: "nanoseconds",
    })
}

enum ExpectedOutcome {
    Integer(i64),
    IntegerRangeVector(usize),
    IntegerIdentityMap(usize),
}

fn measure_one_compiled_roadmap(
    workload_id: &str,
    label: &str,
    expected: ExpectedOutcome,
) -> Result<u128> {
    let root = repo_root()?;
    let source = read_repo_file(&root, roadmap_source_path(workload_id)?)?;
    let forms = canonicalize_module(
        parse_module(&source).with_context(|| format!("parse {label} roadmap workload"))?,
    )
    .with_context(|| format!("canonicalize {label} roadmap workload"))?;
    let compiled =
        compile_module(&forms).with_context(|| format!("compile {label} roadmap workload"))?;
    let mut setup_ctx = EvalCtx::with_step_limit(None);
    let prelude = build_prelude(&mut setup_ctx);
    let mut ctx = EvalCtx::with_step_limit(None);
    let mut env = Env::with_bindings(&prelude.env, BTreeMap::new());
    let start = Instant::now();
    let output = eval_compiled_module(&mut ctx, &mut env, &compiled)
        .with_context(|| format!("eval {label} roadmap workload"))?;
    let duration_ns = start.elapsed().as_nanos();
    ensure_not_sealed_error(&output).with_context(|| format!("{label} roadmap workload"))?;
    validate_expected_output(&output, expected)
        .with_context(|| format!("validate {label} roadmap result"))?;
    Ok(duration_ns)
}

fn measure_one_selfhost_parse(cfg: &BenchConfig) -> Result<u128> {
    let root = repo_root()?;
    let corpus = cfg
        .workload_selfhost_parse_corpus
        .iter()
        .map(|rel| read_repo_file(&root, rel).map(|source| (rel.clone(), source)))
        .collect::<Result<Vec<_>>>()?;
    if corpus.len() != 2
        || cfg.workload_selfhost_parse_corpus != ["selfhost/parse.gc", "prelude/prelude.gc"]
    {
        bail!("PB-7 requires the exact normalized two-module parser corpus");
    }
    let mut setup_ctx = EvalCtx::with_step_limit(None);
    let prelude = build_prelude(&mut setup_ctx);
    let mut env = prelude.env;
    load_selfhost_parse(&root, &mut setup_ctx, &mut env)?;
    let parse_module_fn = env
        .get("selfhost/parse::parse-module")
        .context("selfhost/parse::parse-module not bound after selfhost frontend load")?;
    let mut ctx = EvalCtx::with_step_limit(None);
    let start = Instant::now();
    for (rel, source) in &corpus {
        parse_source_with_selfhost(&mut ctx, &parse_module_fn, rel, source)?;
    }
    Ok(start.elapsed().as_nanos())
}

fn validate_expected_output(value: &Value, expected: ExpectedOutcome) -> Result<()> {
    match expected {
        ExpectedOutcome::Integer(expected) => {
            if value_to_i64(value) != Some(expected) {
                bail!("expected integer {expected}, got {}", value.debug_repr());
            }
        }
        ExpectedOutcome::IntegerRangeVector(expected_len) => {
            let Value::Vector(items) = value else {
                bail!("expected vector, got {}", value.debug_repr());
            };
            if items.len() != expected_len {
                bail!("expected vector length {expected_len}, got {}", items.len());
            }
            for (index, item) in items.iter().enumerate() {
                if value_to_i64(item) != i64::try_from(index).ok() {
                    bail!("vector value mismatch at index {index}");
                }
            }
        }
        ExpectedOutcome::IntegerIdentityMap(expected_len) => {
            let Value::Map(entries) = value else {
                bail!("expected map, got {}", value.debug_repr());
            };
            if entries.size() != expected_len {
                bail!("expected map size {expected_len}, got {}", entries.size());
            }
            for (index, (key, item)) in entries.iter().enumerate() {
                let key_value = match &key.0 {
                    Term::Int(value) => value.to_string().parse::<i64>().ok(),
                    _ => None,
                };
                let expected_value = i64::try_from(index).ok();
                if key_value != expected_value || value_to_i64(item) != expected_value {
                    bail!("map identity mismatch at ordered entry {index}");
                }
            }
        }
    }
    Ok(())
}

fn value_to_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Int(value) => Some(*value),
        Value::Data(term) => match term.as_ref() {
            Term::Int(value) => value.to_string().parse().ok(),
            _ => None,
        },
        _ => None,
    }
}

fn roadmap_source_path(workload_id: &str) -> Result<&'static str> {
    match workload_id {
        "PB-1" => Ok("benchmarks/roadmap/v0.1/pb1_fib_25.gc"),
        "PB-4" => Ok("benchmarks/roadmap/v0.1/pb4_vector_1000000.gc"),
        "PB-5" => Ok("benchmarks/roadmap/v0.1/pb5_map_100000.gc"),
        _ => bail!("no normalized source fixture for {workload_id}"),
    }
}

fn workload_source<F>(cfg: &BenchConfig, workload_id: &str, generated: F) -> Result<String>
where
    F: FnOnce() -> String,
{
    if cfg.workload_profile != "roadmap" {
        return Ok(generated());
    }
    let root = repo_root()?;
    read_repo_file(&root, roadmap_source_path(workload_id)?)
}

fn measure_compiled_workload(cfg: &BenchConfig, label: &str, src: &str) -> Result<u128> {
    let forms = canonicalize_module(
        parse_module(src).with_context(|| format!("parse {label} workload module"))?,
    )
    .with_context(|| format!("canonicalize {label} workload module"))?;
    let compiled = compile_module(&forms).with_context(|| format!("compile {label} workload"))?;

    let mut setup_ctx = EvalCtx::with_step_limit(None);
    let prelude = build_prelude(&mut setup_ctx);
    let prelude_env = prelude.env;

    best_of(cfg.warmups, cfg.repeats, || {
        let mut ctx = EvalCtx::with_step_limit(None);
        let mut env = Env::with_bindings(&prelude_env, BTreeMap::new());
        let out = eval_compiled_module(&mut ctx, &mut env, &compiled)
            .with_context(|| format!("eval {label} workload"))?;
        ensure_not_sealed_error(&out)
            .with_context(|| format!("{label} workload returned error"))?;
        Ok(())
    })
}

fn measure_selfhost_parse(cfg: &BenchConfig) -> Result<u128> {
    let root = repo_root()?;
    let corpus = cfg
        .workload_selfhost_parse_corpus
        .iter()
        .map(|rel| read_repo_file(&root, rel).map(|src| (rel.clone(), src)))
        .collect::<Result<Vec<_>>>()?;
    if corpus.is_empty() {
        bail!("selfhost parse workload corpus is empty");
    }

    let mut setup_ctx = EvalCtx::with_step_limit(None);
    let prelude = build_prelude(&mut setup_ctx);
    let mut env = prelude.env;
    load_selfhost_parse(&root, &mut setup_ctx, &mut env)?;
    let parse_module_fn = env
        .get("selfhost/parse::parse-module")
        .context("selfhost/parse::parse-module not bound after selfhost frontend load")?;

    best_of(cfg.warmups, cfg.repeats, || {
        let mut ctx = EvalCtx::with_step_limit(None);
        for (rel, src) in &corpus {
            parse_source_with_selfhost(&mut ctx, &parse_module_fn, rel, src)?;
        }
        Ok(())
    })
}

fn parse_source_with_selfhost(
    ctx: &mut EvalCtx,
    parse_module_fn: &Value,
    label: &str,
    src: &str,
) -> Result<()> {
    let out = parse_module_fn
        .clone()
        .apply(ctx, Value::data(Term::Str(src.to_string())))
        .with_context(|| format!("apply selfhost parse-module to {label}"))?;
    ensure_not_sealed_error(&out)
        .with_context(|| format!("selfhost parse-module error for {label}"))?;
    match value_to_term(&out) {
        Some(Term::Vector(forms)) if !forms.is_empty() => Ok(()),
        Some(other) => {
            bail!("selfhost parse-module for {label} returned non-module term: {other:?}")
        }
        None => bail!(
            "selfhost parse-module for {label} returned non-data value: {}",
            out.debug_repr()
        ),
    }
}

fn load_selfhost_parse(root: &Path, ctx: &mut EvalCtx, env: &mut Env) -> Result<()> {
    for rel in ["selfhost/parse.gc", "selfhost/parse_core_v1.gc"] {
        let src = read_repo_file(root, rel)?;
        let forms =
            canonicalize_module(parse_module(&src).with_context(|| format!("parse {rel}"))?)
                .with_context(|| format!("canonicalize {rel}"))?;
        let compiled = compile_module(&forms).with_context(|| format!("compile {rel}"))?;
        let out =
            eval_compiled_module(ctx, env, &compiled).with_context(|| format!("eval {rel}"))?;
        ensure_not_sealed_error(&out).with_context(|| format!("loading {rel}"))?;
    }
    Ok(())
}

fn repo_root() -> Result<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .context("derive repository root from CARGO_MANIFEST_DIR")
}

fn read_repo_file(root: &Path, rel: &str) -> Result<String> {
    let rel_path = Path::new(rel);
    if rel_path.is_absolute()
        || rel_path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        bail!("workload corpus path must be workspace-relative: {rel}");
    }
    let path = root.join(rel);
    std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))
}

fn value_to_term(v: &Value) -> Option<Term> {
    v.to_plain_term()
}

fn ensure_not_sealed_error(v: &Value) -> Result<()> {
    if matches!(v, Value::Sealed { .. }) {
        bail!("sealed boundary value: {}", v.debug_repr());
    }
    Ok(())
}

fn fib_src(n: usize) -> String {
    format!(
        r#"
(def bench/fib
  (fn (n)
    (if ((core/int::lt? n) 2)
      n
      ((core/int::add (bench/fib ((core/int::sub n) 1)))
        (bench/fib ((core/int::sub n) 2))))))
(bench/fib {n})
"#
    )
}

fn vec_build_src(n: usize) -> String {
    format!(
        r#"
(def bench/vec-build-loop
  (fn (i)
    (fn (n)
      (fn (acc)
        (if ((core/int::eq? i) n)
          acc
          (((bench/vec-build-loop ((core/int::add i) 1)) n)
            ((core/vec::push acc) i)))))))
(((bench/vec-build-loop 0) {n}) [])
"#
    )
}

fn map_build_src(n: usize) -> String {
    format!(
        r#"
(def bench/map-build-loop
  (fn (i)
    (fn (n)
      (fn (acc)
        (if ((core/int::eq? i) n)
          acc
          (((bench/map-build-loop ((core/int::add i) 1)) n)
            (((core/map::put acc) i) i)))))))
(((bench/map-build-loop 0) {n}) {{}})
"#
    )
}

fn str_concat_src(n: usize) -> String {
    format!(
        r#"
(def bench/str-build-loop
  (fn (i)
    (fn (n)
      (fn (acc)
        (if ((core/int::eq? i) n)
          acc
          (((bench/str-build-loop ((core/int::add i) 1)) n)
            ((core/str::concat acc) "x")))))))
(((bench/str-build-loop 0) {n}) "")
"#
    )
}

fn dispatch_src(n: usize) -> String {
    format!(
        r#"
(def bench/msg ((core/msg::make (quote bench/op)) nil))
(def bench/base (core/contract::extend core/contract::genesis {{bench/op (fn (_m) 1)}} {{}}))
(def bench/c1 (core/contract::extend bench/base {{}} {{}}))
(def bench/c2 (core/contract::extend bench/c1 {{}} {{}}))
(def bench/c3 (core/contract::extend bench/c2 {{}} {{}}))
(def bench/c4 (core/contract::extend bench/c3 {{}} {{}}))
(def bench/c5 (core/contract::extend bench/c4 {{}} {{}}))
(def bench/dispatch-loop
  (fn (i)
    (fn (n)
      (fn (acc)
        (if ((core/int::eq? i) n)
          acc
          (((bench/dispatch-loop ((core/int::add i) 1)) n)
            ((core/int::add acc) ((core/contract::dispatch bench/c5) bench/msg))))))))
(((bench/dispatch-loop 0) {n}) 0)
"#
    )
}
