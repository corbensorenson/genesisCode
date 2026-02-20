use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

use gc_coreform::{
    Term, TermOrdKey, canonicalize_module, hash_module, parse_module, parse_term, print_module,
    print_term, print_term_compact,
};
use gc_effects::{CapsPolicy, Decision, EffectLog};
use gc_kernel::{Apply, EvalCtx, MemLimits, SealId, StepLimit, Value, eval_module, eval_term};
use gc_obligations::PackageManifest;
use gc_prelude::{
    SelfhostBootstrapMode, build_prelude, embedded_bootstrap_available,
    load_selfhost_coreform_toolchain_v1_with_mode, selfhost_coreform_toolchain_v1_sources,
};

mod cli_json;
mod cli_schema;
mod cmd_agent_index;
mod cmd_commit;
mod cmd_core;
mod cmd_gc;
mod cmd_pkg;
mod cmd_policy;
mod cmd_refs;
mod cmd_registry;
mod cmd_security_ops;
mod cmd_selfhost;
mod cmd_store;
mod cmd_sync;
mod cmd_vcs;
mod commit_contract;
mod diagnostics;
mod gc_contract;
mod kernel_exec;
mod pkg_abi;
mod pkg_contract;
mod pkg_doctor;
mod pkg_reports;
mod pkg_self_opt;
mod pkg_task_runner;
mod pkg_telemetry;
mod pkg_workspace_ops;
mod policy_config;
mod program_builders;
mod refs_contract;
mod selfhost_bridge;
mod selfhost_frontend;
mod sync_contract;
mod vcs_contract;

use cli_json::*;
use cli_schema::cmd_cli_schema;
use cmd_agent_index::cmd_agent_index;
use cmd_commit::cmd_commit;
use cmd_core::*;
use cmd_gc::cmd_gc;
use cmd_pkg::cmd_pkg;
use cmd_policy::cmd_policy;
use cmd_refs::cmd_refs;
use cmd_registry::cmd_registry;
use cmd_security_ops::*;
use cmd_selfhost::*;
use cmd_store::cmd_store;
use cmd_sync::cmd_sync;
pub(crate) use cmd_vcs::SetRefSpec;
use cmd_vcs::{
    cmd_vcs, extract_pkg_export_bundle_hash, extract_pkg_import_root, extract_pkg_lock_hash,
    extract_pkg_ok_bool, extract_pkg_publish_commit, extract_pkg_snapshot_hash,
    extract_refs_get_hash, extract_refs_list_pairs, extract_refs_set_hash,
    extract_vcs_snapshot_hash, is_hex64, normalize_pkg_add_strategy, parse_local_set_refs,
    parse_pkg_spec, parse_sync_set_refs,
};
use diagnostics::annotate_envelope;
use kernel_exec::eval_module_default;
use policy_config::*;
use program_builders::*;
use selfhost_bridge::*;
use selfhost_frontend::*;

const EX_OK: u8 = 0;
const EX_INTERNAL: u8 = 1;
const EX_PARSE: u8 = 10;
const EX_FMT: u8 = 11;
const EX_EVAL: u8 = 20;
const EX_OBLIGATIONS: u8 = 30;
const EX_REPLAY_MISMATCH: u8 = 40;
const EX_CAPS_DENIED: u8 = 41;
const EX_VERIFY: u8 = 50;
const EX_IO: u8 = 70;

include!("cli_args.rs");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Native,
    Wasi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeProfile {
    Production,
    ParityHarness,
}

static RUNTIME_PROFILE: AtomicU8 = AtomicU8::new(0);

pub(crate) fn runtime_profile() -> RuntimeProfile {
    match RUNTIME_PROFILE.load(Ordering::Relaxed) {
        1 => RuntimeProfile::ParityHarness,
        _ => RuntimeProfile::Production,
    }
}

fn set_runtime_profile(profile: RuntimeProfile) {
    let encoded = match profile {
        RuntimeProfile::Production => 0,
        RuntimeProfile::ParityHarness => 1,
    };
    RUNTIME_PROFILE.store(encoded, Ordering::Relaxed);
}

pub fn run(flavor: Flavor) -> std::process::ExitCode {
    run_with_profile(flavor, RuntimeProfile::Production)
}

pub fn run_with_profile(flavor: Flavor, profile: RuntimeProfile) -> std::process::ExitCode {
    set_runtime_profile(profile);
    let parity = matches!(profile, RuntimeProfile::ParityHarness);
    gc_prelude::set_bootstrap_runtime_profile_parity_harness(parity);
    gc_obligations::set_frontend_runtime_profile_parity_harness(parity);
    gc_effects::set_force_wasi_remote_profile(matches!(flavor, Flavor::Wasi));
    let cli = Cli::parse();
    match dispatch(&cli, flavor) {
        Ok(out) => {
            if cli.json {
                // JSON mode: exactly one JSON object on stdout.
                println!("{}", json_canonical_string(&out.json));
            } else if !out.stdout.is_empty() {
                print!("{}", out.stdout);
            }
            std::process::ExitCode::from(out.exit_code)
        }
        Err(e) => {
            if cli.json {
                let out = match json_envelope_value(JsonEnvelope::<serde_json::Value> {
                    ok: false,
                    kind: "genesis/error-v0.2",
                    data: None,
                    error: Some(e.json),
                }) {
                    Ok(v) => v,
                    Err(serr) => serde_json::json!({
                        "ok": false,
                        "kind": "genesis/error-v0.2",
                        "error": {
                            "code": serr.json.code,
                            "message": serr.json.message,
                            "context": serr.json.context,
                        },
                    }),
                };
                let out = annotate_envelope(out, e.exit_code);
                println!("{}", json_canonical_string(&out));
            } else {
                eprintln!("{}", e.json.message);
                if let Some(ctx) = e.json.context
                    && let Some(s) = ctx.as_str()
                    && !s.is_empty()
                {
                    eprintln!("{s}");
                }
            }
            std::process::ExitCode::from(e.exit_code)
        }
    }
}

#[derive(Debug, Deserialize)]
struct WarmRequest {
    argv: Vec<String>,
}

#[derive(Debug)]
struct CmdOut {
    exit_code: u8,
    stdout: String,
    json: serde_json::Value,
}

fn dispatch(cli: &Cli, flavor: Flavor) -> Result<CmdOut, CliError> {
    enforce_selfhost_only_cmd(cli, flavor)?;
    let mut out = match &cli.cmd {
        Cmd::Fmt {
            file,
            check,
            engine,
        } => cmd_fmt(cli, file, *check, *engine),
        Cmd::Eval {
            file,
            engine,
            stage1_pipeline,
            stage1_gate,
            stage2_gate,
        } => cmd_eval(
            cli,
            file,
            *engine,
            *stage1_pipeline,
            *stage1_gate,
            *stage2_gate,
        ),
        Cmd::Explain {
            file,
            engine,
            contract,
            msg,
        } => cmd_explain(cli, file, *engine, contract, msg),
        Cmd::Run {
            file,
            engine,
            caps,
            log,
        } => cmd_run(cli, flavor, file, *engine, caps, log.as_deref()),
        Cmd::Replay {
            file,
            engine,
            log,
            store,
        } => cmd_replay(cli, file, *engine, log, store.as_deref()),
        Cmd::Test { pkg, caps } => cmd_test(cli, pkg, caps.as_deref()),
        Cmd::Pack { pkg } => cmd_pack(cli, pkg),
        Cmd::SelfhostArtifact {
            out,
            min_stage2_supported_modules,
            min_stage2_validated_modules,
        } => cmd_selfhost_artifact(
            cli,
            out,
            *min_stage2_supported_modules,
            *min_stage2_validated_modules,
        ),
        Cmd::SelfhostDashboard { markdown, store } => {
            cmd_selfhost_dashboard(cli, markdown.as_deref(), store.as_deref())
        }
        Cmd::Warm { prime_selfhost } => cmd_warm(cli, flavor, *prime_selfhost),
        Cmd::CliSchema => cmd_cli_schema(cli),
        Cmd::AgentIndex => cmd_agent_index(cli),
        Cmd::Keygen { out } => cmd_keygen(cli, out),
        Cmd::Sign {
            pkg,
            key,
            acceptance,
            signatures,
        } => cmd_sign(cli, pkg, key, acceptance.as_deref(), signatures.as_deref()),
        Cmd::TransparencyVerify { pkg } => cmd_transparency_verify(cli, pkg),
        Cmd::Typecheck { pkg } => cmd_typecheck(cli, pkg),
        Cmd::Optimize {
            file,
            out,
            emit_wasm,
            engine,
            stage1_gate,
            stage2_gate,
        } => cmd_optimize(
            cli,
            file,
            out.as_ref(),
            emit_wasm.as_ref(),
            *engine,
            *stage1_gate,
            *stage2_gate,
        ),
        Cmd::ApplyPatch { patch, pkg, caps } => cmd_apply_patch(cli, patch, pkg, caps.as_deref()),
        Cmd::SemanticEdit { cmd } => cmd_semantic_edit(cli, cmd),
        Cmd::Verify {
            pkg,
            acceptance,
            policy,
            signatures,
            scan_store,
        } => cmd_verify(
            cli,
            pkg,
            acceptance.as_deref(),
            policy.as_deref(),
            signatures.as_deref(),
            *scan_store,
        ),
        Cmd::Store { caps, log, cmd } => cmd_store(cli, caps, log.as_deref(), cmd),
        Cmd::Refs { caps, log, cmd } => cmd_refs(cli, caps, log.as_deref(), cmd),
        Cmd::Commit { caps, log, cmd } => cmd_commit(cli, caps, log.as_deref(), cmd),
        Cmd::Pkg { caps, log, cmd } => cmd_pkg(cli, flavor, caps, log.as_deref(), cmd),
        Cmd::Policy { cmd } => cmd_policy(cli, cmd),
        Cmd::Sync { caps, log, cmd } => cmd_sync(cli, caps, log.as_deref(), cmd),
        Cmd::Registry { cmd } => cmd_registry(cli, flavor, cmd),
        Cmd::Gc { caps, log, cmd } => cmd_gc(cli, caps, log.as_deref(), cmd),
        Cmd::Vcs { caps, log, cmd } => cmd_vcs(cli, caps.as_deref(), log.as_deref(), cmd),
    }?;
    out.json = annotate_envelope(out.json, out.exit_code);
    Ok(out)
}

fn resolved_step_limit(cli: &Cli) -> StepLimit {
    if cli.no_step_limit {
        return StepLimit::Unlimited;
    }
    if let Some(n) = cli.step_limit {
        return StepLimit::Limit(n);
    }
    StepLimit::Default
}

fn resolved_mem_limits(cli: &Cli) -> MemLimits {
    MemLimits {
        max_pair_cells: cli.max_pair_cells,
        max_vec_len: cli.max_vec_len,
        max_map_len: cli.max_map_len,
        max_bytes_len: cli.max_bytes_len,
        max_string_len: cli.max_string_len,
    }
}

fn mk_ctx(cli: &Cli) -> EvalCtx {
    let mut ctx = EvalCtx::with_step_limit(resolved_step_limit(cli).resolve());
    ctx.set_mem_limits(resolved_mem_limits(cli));
    ctx
}

fn cli_err(exit_code: u8, code: &'static str, message: impl Into<String>) -> CliError {
    CliError {
        exit_code,
        json: JsonError {
            code,
            message: message.into(),
            context: None,
        },
    }
}

fn cli_err_anyhow(exit_code: u8, code: &'static str, err: anyhow::Error) -> CliError {
    // Preserve the full anyhow chain so JSON diagnostics show the real root cause.
    cli_err(exit_code, code, format!("{err:#}"))
}

fn caps_parse_cli_err(err: anyhow::Error) -> CliError {
    cli_err_anyhow(EX_PARSE, "caps/parse", err)
}

fn inherited_global_args(cli: &Cli) -> Vec<String> {
    let mut out = Vec::new();
    if cli.json {
        out.push("--json".to_string());
    }
    if let Some(n) = cli.step_limit {
        out.push("--step-limit".to_string());
        out.push(n.to_string());
    }
    if cli.no_step_limit {
        out.push("--no-step-limit".to_string());
    }
    if let Some(n) = cli.max_pair_cells {
        out.push("--max-pair-cells".to_string());
        out.push(n.to_string());
    }
    if let Some(n) = cli.max_vec_len {
        out.push("--max-vec-len".to_string());
        out.push(n.to_string());
    }
    if let Some(n) = cli.max_map_len {
        out.push("--max-map-len".to_string());
        out.push(n.to_string());
    }
    if let Some(n) = cli.max_bytes_len {
        out.push("--max-bytes-len".to_string());
        out.push(n.to_string());
    }
    if let Some(n) = cli.max_string_len {
        out.push("--max-string-len".to_string());
        out.push(n.to_string());
    }
    if let Some(p) = &cli.selfhost_artifact {
        out.push("--selfhost-artifact".to_string());
        out.push(p.display().to_string());
    }
    out.push("--selfhost-bootstrap".to_string());
    out.push(cli.selfhost_bootstrap.as_str().to_string());
    if cli.selfhost_only {
        out.push("--selfhost-only".to_string());
    }
    if let Some(frontend) = cli.coreform_frontend {
        out.push("--coreform-frontend".to_string());
        out.push(frontend.as_str().to_string());
    }
    out
}

fn emit_warm_line(v: &serde_json::Value) -> Result<(), CliError> {
    let mut out = io::stdout().lock();
    writeln!(out, "{}", json_canonical_string(v))
        .map_err(|e| cli_err(EX_IO, "io/error", format!("{e}")))?;
    out.flush()
        .map_err(|e| cli_err(EX_IO, "io/error", format!("{e}")))?;
    Ok(())
}

fn flavor_token(flavor: Flavor) -> &'static str {
    match flavor {
        Flavor::Native => "native",
        Flavor::Wasi => "wasi",
    }
}

fn warm_session_cache_key(
    cli: &Cli,
    flavor: Flavor,
    prime_selfhost: bool,
    inherited: &[String],
) -> String {
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| ".".to_string());
    let payload = serde_json::json!({
        "kind": "genesis/warm-cache-key-v0.1",
        "flavor": flavor_token(flavor),
        "prime_selfhost": prime_selfhost,
        "selfhost_only": cli.selfhost_only,
        "selfhost_bootstrap": cli.selfhost_bootstrap.as_str(),
        "coreform_frontend": cli.coreform_frontend.map(|v| v.as_str()),
        "selfhost_artifact": cli.selfhost_artifact.as_ref().map(|p| p.display().to_string()),
        "cwd": cwd,
        "inherited": inherited,
    });
    let canon = json_canonical_string(&payload);
    let mut h = blake3::Hasher::new();
    h.update(b"GCv0.2\0warm-cache-key\0");
    h.update(canon.as_bytes());
    h.finalize().to_hex().to_string()
}

fn cmd_warm(cli: &Cli, flavor: Flavor, prime_selfhost: bool) -> Result<CmdOut, CliError> {
    if prime_selfhost {
        let frontend = resolved_coreform_frontend(cli)?;
        if matches!(frontend, gc_obligations::CoreformFrontend::Selfhost(_)) {
            let mut ctx = mk_ctx(cli);
            let prelude = build_prelude(&mut ctx);
            let mut env = prelude.env;
            load_runtime_selfhost_toolchain(cli, &mut ctx, &mut env)?;
        }
    }

    let inherited = inherited_global_args(cli);
    let session_cache_key = warm_session_cache_key(cli, flavor, prime_selfhost, &inherited);
    let mut handled: u64 = 0;
    for line in io::stdin().lock().lines() {
        let line = line.map_err(|e| cli_err(EX_IO, "io/error", format!("{e}")))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let req: WarmRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                emit_warm_line(&serde_json::json!({
                    "ok": false,
                    "kind": "genesis/warm-response-v0.1",
                    "error": { "code": "warm/request-parse", "message": format!("{e}") },
                    "data": {
                        "request_index": handled,
                        "session_cache_key": session_cache_key
                    }
                }))?;
                handled = handled.saturating_add(1);
                continue;
            }
        };

        if req.argv.len() == 1 && matches!(req.argv[0].as_str(), "exit" | "quit" | "stop") {
            break;
        }
        if req.argv.first().map(|s| s.as_str()) == Some("warm") {
            emit_warm_line(&serde_json::json!({
                "ok": false,
                "kind": "genesis/warm-response-v0.1",
                "error": { "code": "warm/nested", "message": "nested warm command is not allowed" },
                "data": {
                    "request_index": handled,
                    "session_cache_key": session_cache_key
                }
            }))?;
            handled = handled.saturating_add(1);
            continue;
        }

        let argv: Vec<String> = std::iter::once("genesis".to_string())
            .chain(inherited.iter().cloned())
            .chain(req.argv.iter().cloned())
            .collect();
        let sub_cli = match Cli::try_parse_from(argv) {
            Ok(c) => c,
            Err(e) => {
                emit_warm_line(&serde_json::json!({
                    "ok": false,
                    "kind": "genesis/warm-response-v0.1",
                    "error": { "code": "warm/request-argv-parse", "message": e.to_string() },
                    "data": {
                        "request_index": handled,
                        "session_cache_key": session_cache_key
                    }
                }))?;
                handled = handled.saturating_add(1);
                continue;
            }
        };

        match dispatch(&sub_cli, flavor) {
            Ok(out) => {
                emit_warm_line(&serde_json::json!({
                    "ok": true,
                    "kind": "genesis/warm-response-v0.1",
                    "data": {
                        "request_index": handled,
                        "session_cache_key": session_cache_key,
                        "exit_code": out.exit_code,
                        "result": out.json
                    }
                }))?;
            }
            Err(e) => {
                emit_warm_line(&serde_json::json!({
                    "ok": false,
                    "kind": "genesis/warm-response-v0.1",
                    "error": e.json,
                    "data": {
                        "request_index": handled,
                        "session_cache_key": session_cache_key,
                        "exit_code": e.exit_code
                    }
                }))?;
            }
        }
        handled = handled.saturating_add(1);
    }

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/warm-session-v0.1",
        data: Some(serde_json::json!({
            "requests_handled": handled,
            "prime_selfhost": prime_selfhost,
            "session_cache_key": session_cache_key
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: String::new(),
        json: json_envelope_value(env)?,
    })
}

fn obligation_err(e: gc_obligations::ObligationError) -> CliError {
    match e {
        gc_obligations::ObligationError::Manifest(s) => cli_err(EX_PARSE, "manifest/error", s),
        gc_obligations::ObligationError::Module(s) => cli_err(EX_PARSE, "module/error", s),
        gc_obligations::ObligationError::Test(s) => cli_err(EX_EVAL, "test/error", s),
        gc_obligations::ObligationError::Typecheck(s) => cli_err(EX_EVAL, "typecheck/error", s),
        gc_obligations::ObligationError::Opt(s) => cli_err(EX_INTERNAL, "opt/error", s),
        gc_obligations::ObligationError::Store(s) => cli_err(EX_INTERNAL, "store/error", s),
        gc_obligations::ObligationError::Io(e) => cli_err(EX_IO, "io/error", format!("{e}")),
    }
}

fn cmd_test(cli: &Cli, pkg: &Path, caps: Option<&Path>) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);

    let r = gc_obligations::test_package_with_step_limit_and_frontend(
        pkg,
        caps,
        resolved_step_limit(cli),
        resolved_mem_limits(cli),
        frontend.clone(),
    )
    .map_err(obligation_err)?;
    let exit_code = if r.ok { EX_OK } else { EX_OBLIGATIONS };

    let obligations: Vec<serde_json::Value> = r
        .obligation_results
        .iter()
        .map(|o| {
            serde_json::json!({
                "name": o.name,
                "ok": o.ok,
                "artifact": o.artifact,
                "errors": o.errors,
            })
        })
        .collect();

    let env = JsonEnvelope {
        ok: r.ok,
        kind: "genesis/test-v0.2",
        data: Some(serde_json::json!({
            "pkg": pkg.display().to_string(),
            "caps": caps.map(|p| p.display().to_string()),
            "coreform_frontend": frontend_info,
            "kernel_eval_backend_default": "compiled",
            "acceptance_artifact": r.acceptance_artifact,
            "obligations": obligations,
        })),
        error: None,
    };

    Ok(CmdOut {
        exit_code,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{}\n", r.acceptance_artifact)
        },
        json: json_envelope_value(env)?,
    })
}

fn cmd_pack(cli: &Cli, pkg: &Path) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);

    let h = gc_obligations::pack_with_frontend(pkg, frontend).map_err(obligation_err)?;
    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/pack-v0.2",
        data: Some(serde_json::json!({
            "pkg": pkg.display().to_string(),
            "coreform_frontend": frontend_info,
            "package_artifact": h,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{h}\n")
        },
        json: json_envelope_value(env)?,
    })
}

#[derive(Clone, Copy)]
struct SelfhostCutoverRow {
    cmd: &'static str,
    fast_path_required: bool,
    selfhost_routed: bool,
    default_selfhost: bool,
}

const SELFHOST_CUTOVER_ROWS: &[SelfhostCutoverRow] = &[
    SelfhostCutoverRow {
        cmd: "fmt",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "eval",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "explain",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "run",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "replay",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "test",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "pack",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "selfhost-artifact",
        fast_path_required: false,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "selfhost-dashboard",
        fast_path_required: false,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "agent-index",
        fast_path_required: false,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "keygen",
        fast_path_required: false,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "sign",
        fast_path_required: false,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "transparency-verify",
        fast_path_required: false,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "typecheck",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "optimize",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "apply-patch",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "semantic-edit",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "verify",
        fast_path_required: false,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "store/*",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "refs/*",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "pkg/*",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "policy/*",
        fast_path_required: false,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "sync/*",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "gc/*",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
    SelfhostCutoverRow {
        cmd: "vcs/*",
        fast_path_required: true,
        selfhost_routed: true,
        default_selfhost: true,
    },
];

fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n")
}

fn render_value_for_cli(ctx: &EvalCtx, v: &Value) -> (String, &'static str) {
    // Prefer a stable CoreForm-ish representation. For sealed protocol errors we unwrap the
    // payload for readability.
    let protocol_error: Option<SealId> = ctx.protocol.map(|p| p.error);
    let t = v.to_term_for_log(protocol_error);
    (gc_coreform::print_term(&t), "coreform")
}

fn hex32(h: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::new();
    for b in h {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        EX_PARSE, EX_VERIFY, SelfhostBootstrapMode, enforce_bootstrap_mode_allowed_with_flag,
        json_canonical_string, parse_sync_set_refs,
    };
    use crate::cmd_vcs::parse_set_ref_spec;

    #[test]
    fn parse_set_ref_spec_supports_contract_refs_with_colons() {
        let commit = "a".repeat(64);
        let policy = "b".repeat(64);
        let expected_old = "c".repeat(64);
        let spec = format!(
            "refs/contracts/my-lib/counter::Counter/heads/dev:{commit}:{policy}@{expected_old}"
        );
        let parsed = parse_set_ref_spec(&spec).expect("parse");
        assert_eq!(
            parsed.name,
            "refs/contracts/my-lib/counter::Counter/heads/dev"
        );
        assert_eq!(parsed.hash, commit);
        assert_eq!(parsed.policy, policy);
        assert_eq!(parsed.expected_old.as_deref(), Some(expected_old.as_str()));
    }

    #[test]
    fn parse_set_ref_spec_rejects_invalid_hashes() {
        let err = parse_set_ref_spec("refs/heads/main:nothex:alsonothex").expect_err("must fail");
        assert_eq!(err.exit_code, EX_PARSE);
    }

    #[test]
    fn parse_set_ref_spec_accepts_expected_old_nil() {
        let commit = "a".repeat(64);
        let policy = "b".repeat(64);
        let spec = format!("refs/heads/main:{commit}:{policy}@nil");
        let parsed = parse_set_ref_spec(&spec).expect("parse");
        assert_eq!(parsed.expected_old.as_deref(), Some("nil"));
    }

    #[test]
    fn parse_set_ref_spec_supports_contract_refs_without_expected_old() {
        let commit = "a".repeat(64);
        let policy = "b".repeat(64);
        let spec = format!("refs/contracts/p::q/heads/dev:{commit}:{policy}");
        let parsed = parse_set_ref_spec(&spec).expect("parse");
        assert_eq!(parsed.name, "refs/contracts/p::q/heads/dev");
        assert_eq!(parsed.hash, commit);
        assert_eq!(parsed.policy, policy);
        assert_eq!(parsed.expected_old, None);
    }

    #[test]
    fn json_canonical_string_sorts_object_keys_recursively() {
        let value = serde_json::json!({
            "z": 1,
            "a": {
                "y": 2,
                "x": [{"b": 1, "a": 2}]
            }
        });
        let s = json_canonical_string(&value);
        assert_eq!(s, r#"{"a":{"x":[{"a":2,"b":1}],"y":2},"z":1}"#);
    }

    #[test]
    fn parse_sync_set_refs_rejects_duplicate_targets() {
        let commit = "a".repeat(64);
        let policy = "b".repeat(64);
        let specs = vec![
            format!("refs/heads/main:{commit}:{policy}"),
            format!("refs/heads/main:{commit}:{policy}@nil"),
        ];
        let err = parse_sync_set_refs(&specs).expect_err("must fail");
        assert_eq!(err.exit_code, EX_PARSE);
    }

    #[test]
    fn non_artifact_bootstrap_mode_is_dev_only() {
        let err = enforce_bootstrap_mode_allowed_with_flag(
            SelfhostBootstrapMode::Embedded,
            "test",
            false,
        )
        .expect_err("embedded bootstrap should be rejected outside development mode");
        assert_eq!(err.exit_code, EX_VERIFY);
        assert!(err.json.message.contains("development-only"));
        enforce_bootstrap_mode_allowed_with_flag(SelfhostBootstrapMode::Embedded, "test", true)
            .expect("embedded bootstrap should be allowed in development mode");
    }
}
