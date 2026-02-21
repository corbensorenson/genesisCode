use std::io::{self, BufRead, Write};

use super::*;

#[derive(Debug, Deserialize)]
struct WarmRequest {
    argv: Vec<String>,
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

pub(super) fn cmd_warm(
    cli: &Cli,
    flavor: Flavor,
    prime_selfhost: bool,
) -> Result<CmdOut, CliError> {
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
