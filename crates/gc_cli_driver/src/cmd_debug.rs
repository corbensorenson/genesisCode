use std::collections::BTreeMap;
use std::path::Path;

use super::*;

#[derive(Debug, Clone)]
struct DebugTraceBundle {
    file: String,
    engine: &'static str,
    kernel_eval_backend: &'static str,
    contract: String,
    msg: String,
    trace_term: Term,
    trace_hash_hex: String,
    trace_out: Option<String>,
}

#[derive(Debug, Clone)]
struct StepMatcher {
    key_symbol: String,
    expected: Term,
}

pub(super) fn cmd_debug(cli: &Cli, cmd: &DebugCmd) -> Result<CmdOut, CliError> {
    match cmd {
        DebugCmd::Step {
            trace,
            cursor,
            count,
        } => cmd_debug_step(cli, trace, *cursor, *count),
        DebugCmd::Break {
            trace,
            start,
            match_key,
            match_value,
        } => cmd_debug_break(cli, trace, *start, match_key, match_value),
        DebugCmd::Inspect { trace, index } => cmd_debug_inspect(cli, trace, *index),
        DebugCmd::Continue {
            trace,
            cursor,
            match_key,
            match_value,
        } => cmd_debug_continue(
            cli,
            trace,
            *cursor,
            match_key.as_deref(),
            match_value.as_deref(),
        ),
        DebugCmd::Frames {
            trace,
            start,
            limit,
        } => cmd_debug_frames(cli, trace, *start, *limit),
        DebugCmd::Timeline {
            trace,
            layers,
            start,
            limit,
            out,
        } => cmd_debug_timeline(cli, trace, layers, *start, *limit, out.as_deref()),
        DebugCmd::Bisect {
            baseline,
            candidate,
        } => cmd_debug_bisect(cli, baseline, candidate),
    }
}

fn cmd_debug_step(
    cli: &Cli,
    trace_args: &DebugTraceArgs,
    cursor: u64,
    count: u64,
) -> Result<CmdOut, CliError> {
    if count == 0 {
        return Err(cli_err(EX_PARSE, "debug/step", "--count must be >= 1"));
    }
    let bundle = build_debug_trace(cli, trace_args, "debug step")?;
    let steps = extract_trace_steps(&bundle.trace_term)?;
    let cursor_start = parse_cursor(cursor, steps.len(), "debug/step", true)?;
    let step_count = usize::try_from(count).map_err(|_| {
        cli_err(
            EX_PARSE,
            "debug/step",
            format!("--count `{count}` does not fit on this host"),
        )
    })?;
    let cursor_end = cursor_start.saturating_add(step_count).min(steps.len());
    let executed = cursor_end.saturating_sub(cursor_start);
    let selected_index = if executed > 0 {
        Some(cursor_end - 1)
    } else {
        None
    };
    let selected = selected_index.map(|idx| steps[idx].clone());
    let selected_render = selected
        .as_ref()
        .map(gc_coreform::print_term)
        .unwrap_or_else(|| "nil".to_string());

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/debug-step-v0.1",
        data: Some(serde_json::json!({
            "file": bundle.file,
            "engine": bundle.engine,
            "kernel_eval_backend": bundle.kernel_eval_backend,
            "contract": bundle.contract,
            "msg": bundle.msg,
            "trace_hash_hex": bundle.trace_hash_hex,
            "trace_out": bundle.trace_out,
            "trace_step_count": steps.len(),
            "cursor_start": cursor_start,
            "cursor_end": cursor_end,
            "steps_executed": executed,
            "selected_step_index": selected_index,
            "selected_step": selected_render,
            "selected_step_format": "coreform",
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{selected_render}\n")
        },
        json: json_envelope_value(env)?,
    })
}

fn cmd_debug_break(
    cli: &Cli,
    trace_args: &DebugTraceArgs,
    start: u64,
    match_key: &str,
    match_value: &str,
) -> Result<CmdOut, CliError> {
    let bundle = build_debug_trace(cli, trace_args, "debug break")?;
    let steps = extract_trace_steps(&bundle.trace_term)?;
    let start_index = parse_cursor(start, steps.len(), "debug/break", true)?;
    let matcher = parse_matcher(match_key, match_value)?;
    let hit = find_breakpoint(steps, start_index, &matcher)?;
    let hit_render = hit
        .map(|idx| gc_coreform::print_term(&steps[idx]))
        .unwrap_or_else(|| "nil".to_string());

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/debug-break-v0.1",
        data: Some(serde_json::json!({
            "file": bundle.file,
            "engine": bundle.engine,
            "kernel_eval_backend": bundle.kernel_eval_backend,
            "contract": bundle.contract,
            "msg": bundle.msg,
            "trace_hash_hex": bundle.trace_hash_hex,
            "trace_out": bundle.trace_out,
            "trace_step_count": steps.len(),
            "start": start_index,
            "match_key": matcher.key_symbol,
            "match_value": gc_coreform::print_term(&matcher.expected),
            "match_value_format": "coreform",
            "breakpoint_index": hit,
            "breakpoint_step": hit_render,
            "breakpoint_step_format": "coreform",
            "hit": hit.is_some(),
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{hit_render}\n")
        },
        json: json_envelope_value(env)?,
    })
}

fn cmd_debug_inspect(
    cli: &Cli,
    trace_args: &DebugTraceArgs,
    index: u64,
) -> Result<CmdOut, CliError> {
    let bundle = build_debug_trace(cli, trace_args, "debug inspect")?;
    let steps = extract_trace_steps(&bundle.trace_term)?;
    let idx = parse_required_index(index, steps.len(), "debug/inspect")?;
    let frame = gc_coreform::print_term(&steps[idx]);
    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/debug-inspect-v0.1",
        data: Some(serde_json::json!({
            "file": bundle.file,
            "engine": bundle.engine,
            "kernel_eval_backend": bundle.kernel_eval_backend,
            "contract": bundle.contract,
            "msg": bundle.msg,
            "trace_hash_hex": bundle.trace_hash_hex,
            "trace_out": bundle.trace_out,
            "trace_step_count": steps.len(),
            "index": idx,
            "frame": frame,
            "frame_format": "coreform",
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{frame}\n")
        },
        json: json_envelope_value(env)?,
    })
}

fn cmd_debug_continue(
    cli: &Cli,
    trace_args: &DebugTraceArgs,
    cursor: u64,
    match_key: Option<&str>,
    match_value: Option<&str>,
) -> Result<CmdOut, CliError> {
    let bundle = build_debug_trace(cli, trace_args, "debug continue")?;
    let steps = extract_trace_steps(&bundle.trace_term)?;
    let cursor_start = parse_cursor(cursor, steps.len(), "debug/continue", true)?;
    let matcher = match (match_key, match_value) {
        (None, None) => None,
        (Some(k), Some(v)) => Some(parse_matcher(k, v)?),
        _ => {
            return Err(cli_err(
                EX_PARSE,
                "debug/continue",
                "--match-key and --match-value must be provided together",
            ));
        }
    };

    let (cursor_end, selected_index, halted, reason) = if let Some(m) = &matcher {
        if let Some(idx) = find_breakpoint(steps, cursor_start, m)? {
            (idx + 1, Some(idx), true, "breakpoint")
        } else {
            (steps.len(), None, false, "eof")
        }
    } else if cursor_start < steps.len() {
        (steps.len(), Some(steps.len() - 1), false, "eof")
    } else {
        (steps.len(), None, false, "eof")
    };
    let selected = selected_index
        .map(|idx| gc_coreform::print_term(&steps[idx]))
        .unwrap_or_else(|| "nil".to_string());

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/debug-continue-v0.1",
        data: Some(serde_json::json!({
            "file": bundle.file,
            "engine": bundle.engine,
            "kernel_eval_backend": bundle.kernel_eval_backend,
            "contract": bundle.contract,
            "msg": bundle.msg,
            "trace_hash_hex": bundle.trace_hash_hex,
            "trace_out": bundle.trace_out,
            "trace_step_count": steps.len(),
            "cursor_start": cursor_start,
            "cursor_end": cursor_end,
            "selected_step_index": selected_index,
            "selected_step": selected,
            "selected_step_format": "coreform",
            "halted": halted,
            "halt_reason": reason,
            "match_key": matcher.as_ref().map(|m| m.key_symbol.clone()),
            "match_value": matcher.as_ref().map(|m| gc_coreform::print_term(&m.expected)),
            "match_value_format": matcher.as_ref().map(|_| "coreform"),
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{selected}\n")
        },
        json: json_envelope_value(env)?,
    })
}

fn cmd_debug_frames(
    cli: &Cli,
    trace_args: &DebugTraceArgs,
    start: u64,
    limit: Option<u64>,
) -> Result<CmdOut, CliError> {
    let bundle = build_debug_trace(cli, trace_args, "debug frames")?;
    let steps = extract_trace_steps(&bundle.trace_term)?;
    let start_index = parse_cursor(start, steps.len(), "debug/frames", true)?;
    let max = steps.len().saturating_sub(start_index);
    let limit = match limit {
        Some(v) => usize::try_from(v).map_err(|_| {
            cli_err(
                EX_PARSE,
                "debug/frames",
                format!("--limit `{v}` does not fit on this host"),
            )
        })?,
        None => max,
    };
    let window = limit.min(max);
    let end = start_index + window;
    let frame_vec = Term::Vector(steps[start_index..end].to_vec());
    let frames_render = gc_coreform::print_term(&frame_vec);

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/debug-frames-v0.1",
        data: Some(serde_json::json!({
            "file": bundle.file,
            "engine": bundle.engine,
            "kernel_eval_backend": bundle.kernel_eval_backend,
            "contract": bundle.contract,
            "msg": bundle.msg,
            "trace_hash_hex": bundle.trace_hash_hex,
            "trace_out": bundle.trace_out,
            "trace_step_count": steps.len(),
            "start": start_index,
            "limit": window,
            "end": end,
            "frames": frames_render,
            "frames_format": "coreform",
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{frames_render}\n")
        },
        json: json_envelope_value(env)?,
    })
}

fn cmd_debug_timeline(
    cli: &Cli,
    trace_args: &DebugTraceArgs,
    layer_args: &DebugLayerArtifactArgs,
    start: u64,
    limit: Option<u64>,
    out: Option<&Path>,
) -> Result<CmdOut, CliError> {
    let bundle = build_debug_trace(cli, trace_args, "debug timeline")?;
    let timeline_frames = build_timeline_frames(&bundle, layer_args)?;
    let start_index = parse_cursor(start, timeline_frames.len(), "debug/timeline", true)?;
    let max = timeline_frames.len().saturating_sub(start_index);
    let limit = match limit {
        Some(v) => usize::try_from(v).map_err(|_| {
            cli_err(
                EX_PARSE,
                "debug/timeline",
                format!("--limit `{v}` does not fit on this host"),
            )
        })?,
        None => max,
    };
    let window = limit.min(max);
    let end = start_index + window;
    let timeline_window_term = Term::Vector(timeline_frames[start_index..end].to_vec());
    let timeline_window_render = gc_coreform::print_term(&timeline_window_term);
    let timeline_term = build_timeline_artifact(&bundle, &timeline_frames);
    let timeline_hash_hex = hex32(gc_coreform::hash_term(&timeline_term));
    let timeline_out = write_optional_term_artifact(out, &timeline_term, "debug/timeline")?;
    let layer_counts = timeline_layer_counts(&timeline_frames);

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/debug-timeline-v0.1",
        data: Some(serde_json::json!({
            "file": bundle.file,
            "engine": bundle.engine,
            "kernel_eval_backend": bundle.kernel_eval_backend,
            "contract": bundle.contract,
            "msg": bundle.msg,
            "trace_hash_hex": bundle.trace_hash_hex,
            "trace_out": bundle.trace_out,
            "timeline_hash_hex": timeline_hash_hex,
            "timeline_out": timeline_out,
            "timeline_frame_count": timeline_frames.len(),
            "window_start": start_index,
            "window_end": end,
            "window_count": window,
            "timeline_window": timeline_window_render,
            "timeline_window_format": "coreform",
            "layer_counts": layer_counts,
            "planner_json": layer_args.planner_json.as_ref().map(|p| p.display().to_string()),
            "typecheck_json": layer_args.typecheck_json.as_ref().map(|p| p.display().to_string()),
            "optimize_json": layer_args.optimize_json.as_ref().map(|p| p.display().to_string()),
            "effect_log": layer_args.effect_log.as_ref().map(|p| p.display().to_string()),
        })),
        error: None,
    };

    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{timeline_window_render}\n")
        },
        json: json_envelope_value(env)?,
    })
}

fn cmd_debug_bisect(
    cli: &Cli,
    baseline: &Path,
    candidate: &Path,
) -> Result<CmdOut, CliError> {
    let baseline_term = read_timeline_artifact(baseline, "debug/bisect")?;
    let candidate_term = read_timeline_artifact(candidate, "debug/bisect")?;
    let baseline_frames = extract_timeline_frames(&baseline_term, "debug/bisect baseline")?;
    let candidate_frames = extract_timeline_frames(&candidate_term, "debug/bisect candidate")?;

    let (mismatch_index, reason) = first_timeline_mismatch(baseline_frames, candidate_frames);
    let baseline_frame = mismatch_index
        .and_then(|idx| baseline_frames.get(idx))
        .cloned();
    let candidate_frame = mismatch_index
        .and_then(|idx| candidate_frames.get(idx))
        .cloned();
    let baseline_frame_render = baseline_frame
        .as_ref()
        .map(gc_coreform::print_term)
        .unwrap_or_else(|| "nil".to_string());
    let candidate_frame_render = candidate_frame
        .as_ref()
        .map(gc_coreform::print_term)
        .unwrap_or_else(|| "nil".to_string());
    let baseline_layer = baseline_frame
        .as_ref()
        .and_then(extract_timeline_frame_layer);
    let candidate_layer = candidate_frame
        .as_ref()
        .and_then(extract_timeline_frame_layer);

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/debug-bisect-v0.1",
        data: Some(serde_json::json!({
            "baseline": baseline.display().to_string(),
            "candidate": candidate.display().to_string(),
            "baseline_frame_count": baseline_frames.len(),
            "candidate_frame_count": candidate_frames.len(),
            "mismatch_index": mismatch_index,
            "mismatch_reason": reason,
            "baseline_layer": baseline_layer,
            "candidate_layer": candidate_layer,
            "baseline_frame": baseline_frame_render,
            "candidate_frame": candidate_frame_render,
            "frame_format": "coreform",
            "match": mismatch_index.is_none(),
        })),
        error: None,
    };

    let stdout = if cli.json {
        String::new()
    } else if mismatch_index.is_none() {
        "match\n".to_string()
    } else {
        format!("{baseline_frame_render}\n{candidate_frame_render}\n")
    };

    Ok(CmdOut {
        exit_code: EX_OK,
        stdout,
        json: json_envelope_value(env)?,
    })
}

fn build_timeline_frames(
    bundle: &DebugTraceBundle,
    layer_args: &DebugLayerArtifactArgs,
) -> Result<Vec<Term>, CliError> {
    let mut frames: Vec<Term> = Vec::new();
    append_layer_json_frame(
        &mut frames,
        ":planner",
        layer_args.planner_json.as_deref(),
        "debug/timeline",
    )?;
    append_layer_json_frame(
        &mut frames,
        ":typecheck",
        layer_args.typecheck_json.as_deref(),
        "debug/timeline",
    )?;
    append_layer_json_frame(
        &mut frames,
        ":optimizer",
        layer_args.optimize_json.as_deref(),
        "debug/timeline",
    )?;

    let steps = extract_trace_steps(&bundle.trace_term)?;
    let mut req_index: BTreeMap<[u8; 32], usize> = BTreeMap::new();
    for (idx, step) in steps.iter().enumerate() {
        if let Some(req_h) = extract_step_hash_bytes(step, ":req-h") {
            req_index.insert(req_h, idx);
        }
        frames.push(build_dispatch_frame(idx, step));
    }

    if let Some(effect_log_path) = layer_args.effect_log.as_deref() {
        let log = load_effect_log(effect_log_path, "debug/timeline")?;
        for (idx, entry) in log.entries.iter().enumerate() {
            let dispatch_idx = req_index.get(&entry.req_h).copied();
            frames.push(build_effect_frame(idx, entry, dispatch_idx));
        }
    }

    Ok(annotate_timeline_frame_ids(frames))
}

fn append_layer_json_frame(
    frames: &mut Vec<Term>,
    layer: &str,
    path: Option<&Path>,
    code: &'static str,
) -> Result<(), CliError> {
    let Some(path) = path else {
        return Ok(());
    };
    let src = std::fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    let parsed: serde_json::Value = serde_json::from_str(&src)
        .map_err(|e| cli_err(EX_PARSE, code, format!("parse {}: {e}", path.display())))?;
    let canonical = json_canonical_string(&parsed);
    let payload_term = Term::Str(canonical);
    let kind = parsed
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let ok = parsed
        .get("ok")
        .and_then(serde_json::Value::as_bool)
        .map(Term::Bool)
        .unwrap_or(Term::Nil);
    let frame = Term::Map(
        [
            (TermOrdKey(Term::symbol(":layer")), Term::symbol(layer)),
            (
                TermOrdKey(Term::symbol(":source")),
                Term::Str(path.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str(kind.to_string()),
            ),
            (TermOrdKey(Term::symbol(":ok")), ok),
            (TermOrdKey(Term::symbol(":payload")), payload_term.clone()),
            (
                TermOrdKey(Term::symbol(":payload-h")),
                Term::Bytes(gc_coreform::hash_term(&payload_term).to_vec().into()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    frames.push(frame);
    Ok(())
}

fn build_dispatch_frame(index: usize, step: &Term) -> Term {
    let mut map = BTreeMap::new();
    map.insert(
        TermOrdKey(Term::symbol(":layer")),
        Term::symbol(":dispatch"),
    );
    map.insert(
        TermOrdKey(Term::symbol(":dispatch-index")),
        Term::Int((index as i64).into()),
    );
    map.insert(TermOrdKey(Term::symbol(":step")), step.clone());
    map.insert(
        TermOrdKey(Term::symbol(":content-h")),
        Term::Bytes(gc_coreform::hash_term(step).to_vec().into()),
    );
    if let Some(req_h) = extract_step_hash_bytes(step, ":req-h") {
        map.insert(
            TermOrdKey(Term::symbol(":req-h")),
            Term::Bytes(req_h.to_vec().into()),
        );
    }
    Term::Map(map)
}

fn build_effect_frame(
    index: usize,
    entry: &gc_effects::EffectLogEntry,
    dispatch_idx: Option<usize>,
) -> Term {
    let entry_term = entry.to_term();
    let mut map = BTreeMap::new();
    map.insert(TermOrdKey(Term::symbol(":layer")), Term::symbol(":effect"));
    map.insert(
        TermOrdKey(Term::symbol(":effect-index")),
        Term::Int((index as i64).into()),
    );
    map.insert(
        TermOrdKey(Term::symbol(":op")),
        Term::symbol(entry.op.clone()),
    );
    map.insert(
        TermOrdKey(Term::symbol(":decision")),
        Term::symbol(match entry.decision {
            gc_effects::Decision::Allow => ":allow",
            gc_effects::Decision::Deny => ":deny",
        }),
    );
    map.insert(
        TermOrdKey(Term::symbol(":req-h")),
        Term::Bytes(entry.req_h.to_vec().into()),
    );
    map.insert(
        TermOrdKey(Term::symbol(":payload-h")),
        Term::Bytes(entry.payload_h.to_vec().into()),
    );
    map.insert(
        TermOrdKey(Term::symbol(":resp-h")),
        Term::Bytes(entry.resp_h.to_vec().into()),
    );
    map.insert(TermOrdKey(Term::symbol(":entry")), entry_term.clone());
    map.insert(
        TermOrdKey(Term::symbol(":content-h")),
        Term::Bytes(gc_coreform::hash_term(&entry_term).to_vec().into()),
    );
    map.insert(
        TermOrdKey(Term::symbol(":dispatch-index")),
        dispatch_idx
            .map(|v| Term::Int((v as i64).into()))
            .unwrap_or(Term::Nil),
    );
    Term::Map(map)
}

fn annotate_timeline_frame_ids(mut frames: Vec<Term>) -> Vec<Term> {
    for (idx, frame) in frames.iter_mut().enumerate() {
        if let Term::Map(map) = frame {
            map.insert(
                TermOrdKey(Term::symbol(":frame-id")),
                Term::Int((idx as i64).into()),
            );
        }
    }
    frames
}

fn build_timeline_artifact(bundle: &DebugTraceBundle, frames: &[Term]) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":schema")),
                Term::Str("genesis/debug-timeline-v0.1".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":file")),
                Term::Str(bundle.file.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":engine")),
                Term::Str(bundle.engine.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":kernel-eval-backend")),
                Term::Str(bundle.kernel_eval_backend.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":trace-hash-hex")),
                Term::Str(bundle.trace_hash_hex.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":frame-count")),
                Term::Int((frames.len() as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":frames")),
                Term::Vector(frames.to_vec()),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn timeline_layer_counts(frames: &[Term]) -> serde_json::Value {
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for frame in frames {
        if let Some(layer) = extract_timeline_frame_layer(frame) {
            *counts.entry(layer).or_insert(0) += 1;
        }
    }
    serde_json::to_value(counts).unwrap_or_else(|_| serde_json::json!({}))
}

fn extract_timeline_frame_layer(frame: &Term) -> Option<String> {
    let Term::Map(map) = frame else {
        return None;
    };
    let key = TermOrdKey(Term::symbol(":layer"));
    match map.get(&key) {
        Some(Term::Symbol(s)) => Some(s.clone()),
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn extract_step_hash_bytes(step: &Term, key: &str) -> Option<[u8; 32]> {
    let Term::Map(map) = step else {
        return None;
    };
    let k = TermOrdKey(Term::symbol(key));
    let bytes = map.get(&k).and_then(|v| match v {
        Term::Bytes(b) if b.len() == 32 => Some(b.as_ref()),
        _ => None,
    })?;
    let mut out = [0u8; 32];
    out.copy_from_slice(bytes);
    Some(out)
}

fn load_effect_log(path: &Path, code: &'static str) -> Result<gc_effects::EffectLog, CliError> {
    let src = std::fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    let log_term =
        parse_term(&src).map_err(|e| cli_err(EX_PARSE, code, format!("parse log: {e}")))?;
    gc_effects::EffectLog::from_term(&log_term)
        .map_err(|e| cli_err(EX_PARSE, code, format!("decode log: {e}")))
}

fn write_optional_term_artifact(
    out: Option<&Path>,
    term: &Term,
    code: &'static str,
) -> Result<Option<String>, CliError> {
    let Some(path) = out else {
        return Ok(None);
    };
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mkdir {}", parent.display()))
            .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;
    }
    std::fs::write(path, format!("{}\n", gc_coreform::print_term(term)))
        .with_context(|| format!("write {}", path.display()))
        .map_err(|e| cli_err(EX_IO, code, format!("{e}")))?;
    Ok(Some(path.display().to_string()))
}

fn read_timeline_artifact(path: &Path, code: &'static str) -> Result<Term, CliError> {
    let src = std::fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    parse_term(&src).map_err(|e| cli_err(EX_PARSE, code, format!("parse timeline: {e}")))
}

fn extract_timeline_frames<'a>(
    timeline: &'a Term,
    code: &'static str,
) -> Result<&'a Vec<Term>, CliError> {
    let Term::Map(map) = timeline else {
        return Err(cli_err(EX_PARSE, code, "timeline artifact must be a map"));
    };
    let Some(Term::Vector(frames)) = map.get(&TermOrdKey(Term::symbol(":frames"))) else {
        return Err(cli_err(
            EX_PARSE,
            code,
            "timeline artifact missing :frames vector",
        ));
    };
    for (idx, frame) in frames.iter().enumerate() {
        if !matches!(frame, Term::Map(_)) {
            return Err(cli_err(
                EX_PARSE,
                code,
                format!("timeline :frames[{idx}] must be map"),
            ));
        }
    }
    Ok(frames)
}

fn first_timeline_mismatch(
    baseline_frames: &[Term],
    candidate_frames: &[Term],
) -> (Option<usize>, &'static str) {
    let shared = baseline_frames.len().min(candidate_frames.len());
    for idx in 0..shared {
        let left = normalize_timeline_frame_for_compare(&baseline_frames[idx]);
        let right = normalize_timeline_frame_for_compare(&candidate_frames[idx]);
        if left != right {
            return (Some(idx), "frame-mismatch");
        }
    }
    if baseline_frames.len() != candidate_frames.len() {
        return (Some(shared), "length-mismatch");
    }
    (None, "match")
}

fn normalize_timeline_frame_for_compare(frame: &Term) -> Term {
    let Term::Map(map) = frame else {
        return frame.clone();
    };
    let mut out = map.clone();
    out.remove(&TermOrdKey(Term::symbol(":frame-id")));
    Term::Map(out)
}

fn build_debug_trace(
    cli: &Cli,
    trace_args: &DebugTraceArgs,
    cmd_name: &str,
) -> Result<DebugTraceBundle, CliError> {
    let engine = resolved_engine(cli, cmd_name, trace_args.engine)?;
    let src = std::fs::read_to_string(&trace_args.file)
        .with_context(|| format!("read {}", trace_args.file.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    let (mut ctx, mut env, forms, contract_term, msg_term) = match engine {
        #[cfg(feature = "parity-harness")]
        FmtEngine::Rust => {
            let forms = parse_module(&src)
                .map_err(|e| cli_err(EX_PARSE, "parse/coreform", e.to_string()))?;
            let forms = canonicalize_module(forms)
                .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
            let contract_term = parse_term(&trace_args.contract)
                .map_err(|e| cli_err(EX_PARSE, "parse/term", format!("--contract: {e}")))?;
            let msg_term = parse_term(&trace_args.msg)
                .map_err(|e| cli_err(EX_PARSE, "parse/term", format!("--msg: {e}")))?;
            let mut ctx = mk_ctx(cli);
            let prelude = build_prelude(&mut ctx);
            (ctx, prelude.env, forms, contract_term, msg_term)
        }
        FmtEngine::Selfhost => {
            let mut parse_ctx = EvalCtx::with_step_limit(None);
            parse_ctx.set_mem_limits(resolved_mem_limits(cli));
            let prelude = build_prelude(&mut parse_ctx);
            let mut parse_env = prelude.env;
            load_runtime_selfhost_toolchain(cli, &mut parse_ctx, &mut parse_env)?;

            parse_ctx.steps = 0;
            parse_ctx.step_limit = None;
            let forms = selfhost_parse_canonicalize_module(&mut parse_ctx, &parse_env, &src)?;
            let contract_term = selfhost_parse_term(
                &mut parse_ctx,
                &parse_env,
                &trace_args.contract,
                "--contract",
            )?;
            let msg_term =
                selfhost_parse_term(&mut parse_ctx, &parse_env, &trace_args.msg, "--msg")?;

            let mut eval_ctx = mk_ctx(cli);
            let prelude = build_prelude(&mut eval_ctx);
            (eval_ctx, prelude.env, forms, contract_term, msg_term)
        }
    };

    let (_, eval_backend) = eval_module_default(&mut ctx, &mut env, &forms)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;
    let contract = eval_term(&mut ctx, &env, &contract_term)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("--contract: {e}")))?;
    let msg_val = Value::Data(msg_term);
    let explain = env.get("core/contract::explain").ok_or_else(|| {
        cli_err(
            EX_INTERNAL,
            "prelude/missing",
            "missing prelude binding core/contract::explain",
        )
    })?;
    let trace_value = explain
        .apply(&mut ctx, contract)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("apply contract: {e}")))?
        .apply(&mut ctx, msg_val)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("explain failed: {e}")))?;
    let trace_term = trace_value.as_data().cloned().ok_or_else(|| {
        cli_err(
            EX_EVAL,
            "debug/trace",
            "core/contract::explain returned non-data value",
        )
    })?;
    extract_trace_steps(&trace_term)?;
    let trace_hash_hex = hex32(gc_coreform::hash_term(&trace_term));

    let trace_out = if let Some(path) = trace_args.trace_out.as_ref() {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("mkdir {}", parent.display()))
                .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;
        }
        std::fs::write(path, format!("{}\n", gc_coreform::print_term(&trace_term)))
            .with_context(|| format!("write {}", path.display()))
            .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;
        Some(path.display().to_string())
    } else {
        None
    };

    Ok(DebugTraceBundle {
        file: trace_args.file.display().to_string(),
        engine: engine.as_str(),
        kernel_eval_backend: eval_backend.as_str(),
        contract: trace_args.contract.clone(),
        msg: trace_args.msg.clone(),
        trace_term,
        trace_hash_hex,
        trace_out,
    })
}

fn extract_trace_steps(trace: &Term) -> Result<&Vec<Term>, CliError> {
    let Term::Map(map) = trace else {
        return Err(cli_err(
            EX_EVAL,
            "debug/trace",
            "trace artifact must be a map",
        ));
    };
    let key = TermOrdKey(Term::symbol(":steps"));
    let Some(Term::Vector(steps)) = map.get(&key) else {
        return Err(cli_err(
            EX_EVAL,
            "debug/trace",
            "trace artifact missing :steps vector",
        ));
    };
    for (idx, step) in steps.iter().enumerate() {
        if !matches!(step, Term::Map(_)) {
            return Err(cli_err(
                EX_EVAL,
                "debug/trace",
                format!("trace :steps[{idx}] must be map"),
            ));
        }
    }
    Ok(steps)
}

fn parse_cursor(
    value: u64,
    len: usize,
    code: &'static str,
    allow_equal_end: bool,
) -> Result<usize, CliError> {
    let idx = usize::try_from(value).map_err(|_| {
        cli_err(
            EX_PARSE,
            code,
            format!("index `{value}` does not fit on this host"),
        )
    })?;
    let max = if allow_equal_end {
        idx <= len
    } else {
        idx < len
    };
    if !max {
        return Err(cli_err(
            EX_PARSE,
            code,
            format!("index `{idx}` out of range for {len} steps"),
        ));
    }
    Ok(idx)
}

fn parse_required_index(value: u64, len: usize, code: &'static str) -> Result<usize, CliError> {
    if len == 0 {
        return Err(cli_err(EX_PARSE, code, "trace has no steps"));
    }
    parse_cursor(value, len, code, false)
}

fn parse_matcher(match_key: &str, match_value: &str) -> Result<StepMatcher, CliError> {
    let normalized = normalize_step_match_key(match_key)?;
    let expected = parse_term(match_value)
        .map_err(|e| cli_err(EX_PARSE, "debug/match", format!("--match-value: {e}")))?;
    Ok(StepMatcher {
        key_symbol: normalized,
        expected,
    })
}

fn normalize_step_match_key(raw: &str) -> Result<String, CliError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(cli_err(
            EX_PARSE,
            "debug/match",
            "--match-key must not be empty",
        ));
    }
    if trimmed.starts_with(':') {
        Ok(trimmed.to_string())
    } else {
        Ok(format!(":{trimmed}"))
    }
}

fn find_breakpoint(
    steps: &[Term],
    start: usize,
    matcher: &StepMatcher,
) -> Result<Option<usize>, CliError> {
    let key = TermOrdKey(Term::symbol(&matcher.key_symbol));
    for (idx, step) in steps.iter().enumerate().skip(start) {
        let Term::Map(map) = step else {
            return Err(cli_err(
                EX_EVAL,
                "debug/trace",
                format!("trace :steps[{idx}] must be map"),
            ));
        };
        if map.get(&key) == Some(&matcher.expected) {
            return Ok(Some(idx));
        }
    }
    Ok(None)
}
