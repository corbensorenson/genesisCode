use super::*;

struct CanonicalModuleSource {
    source: String,
    canonical: String,
    module_hash: [u8; 32],
    source_start_byte: u64,
    source_end_byte: u64,
    engine: FmtEngine,
}

fn canonical_module_source(
    cli: &Cli,
    file: &PathBuf,
    operation: &'static str,
    engine: Option<FmtEngine>,
) -> Result<CanonicalModuleSource, CliError> {
    #[cfg(feature = "parity-harness")]
    let parse_operation = match operation {
        "parse" => "parse/parse",
        _ => "fmt/parse",
    };
    let engine = resolved_engine(cli, operation, engine)?;
    let source = std::fs::read_to_string(file)
        .with_context(|| format!("read {}", file.display()))
        .map_err(|error| cli_err(EX_IO, "io/read", format!("{error}")))?;

    let frontend = match engine {
        #[cfg(feature = "parity-harness")]
        FmtEngine::Rust => {
            let forms = parse_module(&source).map_err(|error| {
                cli_err_with_context(
                    EX_PARSE,
                    "parse/coreform",
                    error.to_string(),
                    structured_failures::parser_context(parse_operation, file, &source, &error),
                )
            })?;
            let forms = canonicalize_module(forms)
                .map_err(|error| cli_err(EX_PARSE, "canon/coreform", error.to_string()))?;
            CanonicalModuleSource {
                source_start_byte: 0,
                source_end_byte: u64::try_from(source.len()).unwrap_or(u64::MAX),
                module_hash: hash_module(&forms),
                canonical: print_module(&forms),
                source: source.clone(),
                engine,
            }
        }
        FmtEngine::Selfhost => {
            let mut context = EvalCtx::with_step_limit(None);
            context.set_mem_limits(resolved_mem_limits(cli));
            let prelude = build_prelude(&mut context);
            let mut environment = prelude.env;
            load_selfhost_toolchain(cli, &mut context, &mut environment)?;
            context.steps = 0;
            context.step_limit = resolved_step_limit(cli).resolve();
            let result = selfhost_frontend_module_at(&mut context, &environment, &source, file)?;
            CanonicalModuleSource {
                source: source.clone(),
                canonical: result.canonical_source,
                module_hash: result.module_hash,
                source_start_byte: result.source_start_byte,
                source_end_byte: result.source_end_byte,
                engine,
            }
        }
    };
    Ok(frontend)
}

pub(super) fn cmd_parse(
    cli: &Cli,
    file: &PathBuf,
    engine: Option<FmtEngine>,
) -> Result<CmdOut, CliError> {
    let frontend = canonical_module_source(cli, file, "parse", engine)?;
    let changed = normalize_newlines(&frontend.source) != normalize_newlines(&frontend.canonical);
    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/parse-v0.1",
        data: Some(serde_json::json!({
            "file": file.display().to_string(),
            "canonical": !changed,
            "canonical_source": frontend.canonical,
            "frontend_profile": "genesis/coreform-canon-hash-v0.2",
            "module_hash_hex": hex32(frontend.module_hash),
            "source_bytes": frontend.source.len(),
            "source_span": {
                "start_byte": frontend.source_start_byte,
                "end_byte": frontend.source_end_byte,
            },
            "span_unit": "utf8-byte",
            "engine": frontend.engine.as_str(),
            "selfhost_artifact": selfhost_artifact_identity_for_engine(cli, frontend.engine),
        })),
        error: None,
    };
    let json = json_envelope_value(env)?;
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{}\n", json_canonical_string(&json))
        },
        json,
    })
}

pub(super) fn cmd_fmt(
    cli: &Cli,
    file: &PathBuf,
    check: bool,
    engine: Option<FmtEngine>,
) -> Result<CmdOut, CliError> {
    let frontend = canonical_module_source(cli, file, "fmt", engine)?;
    let changed = normalize_newlines(&frontend.source) != normalize_newlines(&frontend.canonical);
    let ok = !check || !changed;
    let exit_code = if ok { EX_OK } else { EX_FMT };

    if !check && changed {
        std::fs::write(file, &frontend.canonical)
            .with_context(|| format!("write {}", file.display()))
            .map_err(|error| cli_err(EX_IO, "io/write", format!("{error}")))?;
    }

    let env = JsonEnvelope {
        ok,
        kind: "genesis/fmt-v0.2",
        data: Some(serde_json::json!({
            "file": file.display().to_string(),
            "check": check,
            "changed": changed,
            "frontend_profile": "genesis/coreform-canon-hash-v0.2",
            "module_hash_hex": hex32(frontend.module_hash),
            "source_span": {
                "start_byte": frontend.source_start_byte,
                "end_byte": frontend.source_end_byte,
            },
            "span_unit": "utf8-byte",
            "engine": frontend.engine.as_str(),
            "selfhost_artifact": selfhost_artifact_identity_for_engine(cli, frontend.engine),
        })),
        error: if ok {
            None
        } else {
            Some(JsonError {
                code: "fmt/not-canonical",
                message: format!("{} is not canonically formatted", file.display()),
                context: None,
            })
        },
    };
    Ok(CmdOut {
        exit_code,
        stdout: String::new(),
        json: json_envelope_value(env)?,
    })
}
