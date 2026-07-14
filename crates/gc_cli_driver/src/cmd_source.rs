use super::*;

fn canonical_module_source(
    cli: &Cli,
    file: &PathBuf,
    operation: &'static str,
    engine: Option<FmtEngine>,
) -> Result<(String, String, FmtEngine), CliError> {
    let (_parse_operation, evaluate_operation) = match operation {
        "parse" => ("parse/parse", "parse/evaluate"),
        _ => ("fmt/parse", "fmt/evaluate"),
    };
    let engine = resolved_engine(cli, operation, engine)?;
    let source = std::fs::read_to_string(file)
        .with_context(|| format!("read {}", file.display()))
        .map_err(|error| cli_err(EX_IO, "io/read", format!("{error}")))?;

    let canonical = match engine {
        #[cfg(feature = "parity-harness")]
        FmtEngine::Rust => {
            let forms = parse_module(&source).map_err(|error| {
                cli_err_with_context(
                    EX_PARSE,
                    "parse/coreform",
                    error.to_string(),
                    structured_failures::parser_context(_parse_operation, file, &source, &error),
                )
            })?;
            let forms = canonicalize_module(forms)
                .map_err(|error| cli_err(EX_PARSE, "canon/coreform", error.to_string()))?;
            print_module(&forms)
        }
        FmtEngine::Selfhost => {
            let mut context = EvalCtx::with_step_limit(None);
            context.set_mem_limits(resolved_mem_limits(cli));
            let prelude = build_prelude(&mut context);
            let mut environment = prelude.env;
            load_selfhost_toolchain(cli, &mut context, &mut environment)?;
            let formatter = environment.get("core/cli::fmt-module").ok_or_else(|| {
                cli_err(
                    EX_INTERNAL,
                    "selfhost/missing",
                    "missing binding core/cli::fmt-module",
                )
            })?;

            context.steps = 0;
            context.step_limit = resolved_step_limit(cli).resolve();
            let result = formatter
                .apply(&mut context, Value::data(Term::Str(source.clone())))
                .map_err(|error| {
                    cli_err_with_context(
                        EX_EVAL,
                        "eval/error",
                        format!("selfhost {operation} failed: {error}"),
                        structured_failures::evaluator_context(evaluate_operation, &error),
                    )
                })?;
            if let Some((code, message, payload)) = extract_protocol_error(&context, &result) {
                return Err(CliError {
                    exit_code: EX_PARSE,
                    json: JsonError {
                        code: "selfhost/error",
                        message: format!("{code}: {message}"),
                        context: payload.map(serde_json::Value::String),
                    },
                });
            }
            let Some(Term::Str(canonical)) = result.as_data() else {
                return Err(cli_err(
                    EX_INTERNAL,
                    "selfhost/bad-return",
                    format!(
                        "selfhost {operation} returned non-string: {}",
                        result.debug_repr()
                    ),
                ));
            };
            canonical.clone()
        }
    };
    Ok((source, canonical, engine))
}

pub(super) fn cmd_parse(
    cli: &Cli,
    file: &PathBuf,
    engine: Option<FmtEngine>,
) -> Result<CmdOut, CliError> {
    let (source, canonical, engine) = canonical_module_source(cli, file, "parse", engine)?;
    let changed = normalize_newlines(&source) != normalize_newlines(&canonical);
    let canonical_identity = {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"GCv0.2\0canonical-module-source-v0.1\0");
        hasher.update(canonical.as_bytes());
        hasher.finalize().to_hex().to_string()
    };
    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/parse-v0.1",
        data: Some(serde_json::json!({
            "file": file.display().to_string(),
            "canonical": !changed,
            "canonical_source": canonical,
            "canonical_source_blake3": canonical_identity,
            "source_bytes": source.len(),
            "engine": engine.as_str(),
            "selfhost_artifact": selfhost_artifact_identity_for_engine(cli, engine),
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
    let (source, canonical, engine) = canonical_module_source(cli, file, "fmt", engine)?;
    let changed = normalize_newlines(&source) != normalize_newlines(&canonical);
    let ok = !check || !changed;
    let exit_code = if ok { EX_OK } else { EX_FMT };

    if !check && changed {
        std::fs::write(file, canonical)
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
            "engine": engine.as_str(),
            "selfhost_artifact": selfhost_artifact_identity_for_engine(cli, engine),
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
