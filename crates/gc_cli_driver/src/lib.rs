use std::collections::{BTreeMap, BTreeSet};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
#[cfg(feature = "parity-harness")]
use std::sync::atomic::{AtomicU8, Ordering};

use anyhow::Context;
use clap::{Args, Parser, Subcommand, ValueEnum};
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

#[cfg(all(feature = "profile-headless", feature = "profile-gpu"))]
compile_error!(
    "gc_cli_driver runtime profile conflict: `profile-headless` cannot be combined with `profile-gpu`"
);
#[cfg(all(feature = "profile-headless", feature = "profile-gfx"))]
compile_error!(
    "gc_cli_driver runtime profile conflict: `profile-headless` cannot be combined with `profile-gfx`"
);
#[cfg(all(feature = "profile-headless", feature = "profile-backend"))]
compile_error!(
    "gc_cli_driver runtime profile conflict: `profile-headless` cannot be combined with `profile-backend`"
);
#[cfg(all(feature = "profile-gpu", feature = "profile-gfx"))]
compile_error!(
    "gc_cli_driver runtime profile conflict: use `profile-backend` instead of combining `profile-gpu` and `profile-gfx`"
);
#[cfg(all(feature = "profile-gpu", feature = "profile-backend"))]
compile_error!(
    "gc_cli_driver runtime profile conflict: `profile-gpu` cannot be combined with `profile-backend`"
);
#[cfg(all(feature = "profile-gfx", feature = "profile-backend"))]
compile_error!(
    "gc_cli_driver runtime profile conflict: `profile-gfx` cannot be combined with `profile-backend`"
);

mod agent_session;
mod cli_json;
mod cli_schema;
mod cmd_agent_index;
mod cmd_agent_lookup;
mod cmd_agent_plan;
mod cmd_agent_task_cards;
mod cmd_bench;
mod cmd_commit;
mod cmd_core;
mod cmd_debug;
mod cmd_gc;
mod cmd_pkg;
mod cmd_policy;
mod cmd_refs;
mod cmd_registry;
mod cmd_security_ops;
mod cmd_selfhost;
mod cmd_source;
mod cmd_store;
mod cmd_sync;
mod cmd_vcs;
mod commit_contract;
mod diagnostics;
mod gc_contract;
mod host_bridge_runtime;
mod kernel_exec;
mod mcp;
mod package_obligation_cmds;
mod pkg_abi;
mod pkg_assurance_ops;
mod pkg_assurance_pack_ops;
mod pkg_caps_templates;
mod pkg_contract;
mod pkg_doctor;
mod pkg_reports;
mod pkg_runtime_profile;
mod pkg_scaffold;
mod pkg_self_opt;
mod pkg_task_runner;
mod pkg_telemetry;
mod pkg_workspace_ops;
mod policy_config;
mod program_builders;
mod refs_contract;
mod repair_hints;
mod runtime_backend_profile;
mod selfhost_bridge;
mod selfhost_frontend;
mod semantic_workspace;
mod session_resources;
mod structured_failures;
mod sync_contract;
mod vcs_contract;
mod vcs_helpers;
mod warm_protocol;
mod warm_request;
mod warm_session;
mod warm_session_config;
mod warm_state;
mod warm_worker;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod warm_worker_process;
mod warm_workspace;

use agent_session::cmd_agent_session;
use cli_json::*;
use cli_schema::cmd_cli_schema;
use cmd_agent_index::cmd_agent_index;
use cmd_agent_plan::cmd_agent_plan;
use cmd_bench::cmd_bench;
use cmd_commit::cmd_commit;
use cmd_core::*;
use cmd_debug::cmd_debug;
use cmd_gc::cmd_gc;
use cmd_pkg::cmd_pkg;
use cmd_policy::cmd_policy;
use cmd_refs::cmd_refs;
use cmd_registry::cmd_registry;
use cmd_security_ops::*;
use cmd_selfhost::*;
use cmd_source::*;
use cmd_store::cmd_store;
use cmd_sync::cmd_sync;
use cmd_vcs::cmd_vcs;
use diagnostics::annotate_envelope;
use kernel_exec::eval_module_default;
use mcp::cmd_mcp;
use package_obligation_cmds::{cmd_pack, cmd_test, obligation_err};
use policy_config::*;
use program_builders::*;
use runtime_backend_profile::{
    active_runtime_backend_profile, gfx_desktop_backend_enabled, gpu_device_backend_enabled,
};
use selfhost_bridge::*;
use selfhost_frontend::*;
pub(crate) use vcs_helpers::SetRefSpec;
use vcs_helpers::{
    extract_pkg_export_bundle_hash, extract_pkg_import_root, extract_pkg_lock_hash,
    extract_pkg_ok_bool, extract_pkg_publish_commit, extract_pkg_snapshot_hash,
    extract_refs_get_hash, extract_refs_list_pairs, extract_refs_set_hash, extract_vcs_commit_hash,
    extract_vcs_patch_hash, extract_vcs_snapshot_hash, is_hex64, normalize_pkg_add_strategy,
    parse_local_set_refs, parse_pkg_spec, parse_sync_set_refs,
};
use warm_session::cmd_warm;
use warm_session_config::WarmOptions;

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
    #[cfg(feature = "parity-harness")]
    ParityHarness,
}

#[cfg(feature = "parity-harness")]
static RUNTIME_PROFILE: AtomicU8 = AtomicU8::new(0);

pub(crate) fn runtime_profile() -> RuntimeProfile {
    #[cfg(feature = "parity-harness")]
    {
        match RUNTIME_PROFILE.load(Ordering::Relaxed) {
            1 => RuntimeProfile::ParityHarness,
            _ => RuntimeProfile::Production,
        }
    }
    #[cfg(not(feature = "parity-harness"))]
    {
        RuntimeProfile::Production
    }
}

#[cfg(feature = "parity-harness")]
fn set_runtime_profile(profile: RuntimeProfile) {
    let encoded = match profile {
        RuntimeProfile::Production => 0,
        #[cfg(feature = "parity-harness")]
        RuntimeProfile::ParityHarness => 1,
    };
    RUNTIME_PROFILE.store(encoded, Ordering::Relaxed);
}

#[cfg(not(feature = "parity-harness"))]
fn set_runtime_profile(_profile: RuntimeProfile) {}

#[cfg(feature = "parity-harness")]
fn configure_profile_flags(parity: bool) {
    gc_prelude::set_bootstrap_runtime_profile_parity_harness(parity);
    gc_obligations::set_frontend_runtime_profile_parity_harness(parity);
}

#[cfg(not(feature = "parity-harness"))]
fn configure_profile_flags(_parity: bool) {}

pub fn run(flavor: Flavor) -> std::process::ExitCode {
    set_runtime_profile(RuntimeProfile::Production);
    configure_profile_flags(false);
    run_configured(flavor)
}

#[cfg(feature = "parity-harness")]
pub fn run_with_profile(flavor: Flavor, profile: RuntimeProfile) -> std::process::ExitCode {
    set_runtime_profile(profile);
    let parity = matches!(profile, RuntimeProfile::ParityHarness);
    configure_profile_flags(parity);
    run_configured(flavor)
}

fn run_configured(flavor: Flavor) -> std::process::ExitCode {
    gc_effects::set_force_wasi_remote_profile(matches!(flavor, Flavor::Wasi));
    if let Some(code) = host_bridge_runtime::maybe_run_host_bridge_mode() {
        return code;
    }
    let cli = Cli::parse();
    let stdio_server = matches!(cli.cmd, Cmd::Mcp { .. });
    match dispatch(&cli, flavor) {
        Ok(out) => {
            if stdio_server {
                // MCP owns stdout for the full process lifetime.
            } else if cli.json {
                println!("{}", json_canonical_string(&out.json));
            } else if out.exit_code != 0 {
                if !out.stdout.is_empty() {
                    print!("{}", out.stdout);
                }
                if let Some(rendered) =
                    diagnostics::render_human_envelope(&out.json, human_render_options())
                {
                    eprintln!("{rendered}");
                }
            } else if !out.stdout.is_empty() {
                print!("{}", out.stdout);
            }
            std::process::ExitCode::from(out.exit_code)
        }
        Err(e) => {
            if stdio_server {
                eprintln!("{}", e.json.message);
            } else if cli.json {
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
                eprintln!(
                    "{}",
                    diagnostics::render_human_error(
                        e.json.code,
                        &e.json.message,
                        e.json.context,
                        e.exit_code,
                        human_render_options(),
                    )
                );
            }
            std::process::ExitCode::from(e.exit_code)
        }
    }
}

fn human_render_options() -> diagnostics::HumanRenderOptions {
    let width = std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(96);
    let no_color = std::env::var_os("NO_COLOR").is_some();
    let force_color = std::env::var("CLICOLOR_FORCE")
        .ok()
        .is_some_and(|value| !value.is_empty() && value != "0");
    diagnostics::HumanRenderOptions {
        width,
        color: !no_color && (force_color || std::io::stderr().is_terminal()),
    }
}

#[derive(Debug)]
struct CmdOut {
    exit_code: u8,
    stdout: String,
    json: serde_json::Value,
}

fn dispatch(cli: &Cli, flavor: Flavor) -> Result<CmdOut, CliError> {
    gc_effects::set_session_effect_ceiling(cli.session_max_effects);
    let _active_backend_profile = active_runtime_backend_profile();
    let _backend_flags = (gpu_device_backend_enabled(), gfx_desktop_backend_enabled());
    enforce_selfhost_only_cmd(cli, flavor)?;
    let mut out = match &cli.cmd {
        Cmd::Parse { file, engine } => cmd_parse(cli, file, *engine),
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
        Cmd::Debug { cmd } => cmd_debug(cli, cmd),
        Cmd::Test { pkg, caps } => cmd_test(cli, pkg, caps.as_deref()),
        Cmd::Pack { pkg } => cmd_pack(cli, pkg),
        Cmd::SelfhostArtifact {
            out,
            min_stage2_supported_modules,
            min_stage2_validated_modules,
            recover_missing_artifact,
        } => cmd_selfhost_artifact(
            cli,
            out,
            *min_stage2_supported_modules,
            *min_stage2_validated_modules,
            *recover_missing_artifact,
        ),
        Cmd::SelfhostDashboard { markdown, store } => {
            cmd_selfhost_dashboard(cli, markdown.as_deref(), store.as_deref())
        }
        Cmd::Warm {
            prime_selfhost,
            max_queue,
            max_frame_bytes,
            max_workspaces,
            workspace_idle_ms,
            max_requests,
            max_wall_ms,
            max_cpu_ms,
            max_steps,
            max_heap_bytes,
            max_output_bytes,
            max_effects,
            max_processes,
            max_disk_bytes,
            max_drain_requests,
            drain_timeout_ms,
            workspace_root,
        } => cmd_warm(
            cli,
            flavor,
            WarmOptions {
                prime_selfhost: *prime_selfhost,
                max_queue: *max_queue,
                max_frame_bytes: *max_frame_bytes,
                max_workspaces: *max_workspaces,
                workspace_idle_ms: *workspace_idle_ms,
                max_requests: *max_requests,
                workspace_root,
                resources: session_resources::SessionResourceOptions {
                    max_wall_ms: *max_wall_ms,
                    max_cpu_ms: *max_cpu_ms,
                    max_steps: *max_steps,
                    max_heap_bytes: *max_heap_bytes,
                    max_output_bytes: *max_output_bytes,
                    max_effects: *max_effects,
                    max_processes: *max_processes,
                    max_disk_bytes: *max_disk_bytes,
                    max_drain_requests: *max_drain_requests,
                    drain_timeout_ms: *drain_timeout_ms,
                },
            },
        ),
        Cmd::Mcp {
            prime_selfhost,
            max_queue,
            max_frame_bytes,
            max_output_bytes,
            max_requests,
            max_roots,
            max_wall_ms,
            max_cpu_ms,
            max_steps,
            max_heap_bytes,
            max_effects,
            max_processes,
            max_disk_bytes,
            max_drain_requests,
            drain_timeout_ms,
            workspace_root,
        } => cmd_mcp(
            cli,
            flavor,
            mcp::McpOptions {
                prime_selfhost: *prime_selfhost,
                max_queue: *max_queue,
                max_frame_bytes: *max_frame_bytes,
                max_output_bytes: *max_output_bytes,
                max_requests: *max_requests,
                max_roots: *max_roots,
                workspace_root,
                resources: session_resources::SessionResourceOptions {
                    max_wall_ms: *max_wall_ms,
                    max_cpu_ms: *max_cpu_ms,
                    max_steps: *max_steps,
                    max_heap_bytes: *max_heap_bytes,
                    max_output_bytes: *max_output_bytes,
                    max_effects: *max_effects,
                    max_processes: *max_processes,
                    max_disk_bytes: *max_disk_bytes,
                    max_drain_requests: *max_drain_requests,
                    drain_timeout_ms: *drain_timeout_ms,
                },
            },
        ),
        Cmd::CliSchema => cmd_cli_schema(cli),
        Cmd::AgentIndex {
            symbol,
            diagnostic,
            search_symbol,
            card,
            max_results,
        } => cmd_agent_index(
            cli,
            symbol.as_deref(),
            diagnostic.as_deref(),
            search_symbol.as_deref(),
            *card,
            *max_results,
        ),
        Cmd::AgentPlan {
            intent,
            caps,
            max_workflows,
        } => cmd_agent_plan(cli, intent, caps, *max_workflows),
        Cmd::Bench { cmd } => cmd_bench(cli, cmd),
        Cmd::Keygen { out } => cmd_keygen(cli, out),
        Cmd::Sign {
            pkg,
            key,
            acceptance,
            signatures,
        } => cmd_sign(cli, pkg, key, acceptance.as_deref(), signatures.as_deref()),
        Cmd::TransparencyVerify { pkg } => cmd_transparency_verify(cli, pkg),
        Cmd::Typecheck { pkg, strict_sound } => cmd_typecheck(cli, pkg, *strict_sound),
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
        Cmd::Session { cmd } => cmd_agent_session(cli, cmd),
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

fn cli_err_with_context(
    exit_code: u8,
    code: &'static str,
    message: impl Into<String>,
    context: serde_json::Value,
) -> CliError {
    CliError {
        exit_code,
        json: JsonError {
            code,
            message: message.into(),
            context: Some(context),
        },
    }
}

fn caps_parse_cli_err(err: anyhow::Error) -> CliError {
    let message = format!("{err:#}");
    let context = structured_failures::FailureContext::new(
        "policy",
        "capability-manifest-parse",
        "policy/load-capabilities",
    )
    .fact("reason", message.clone())
    .into_value();
    cli_err_with_context(EX_PARSE, "caps/parse", message, context)
}

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
#[path = "lib_tests.rs"]
mod tests;
