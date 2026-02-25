use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey, parse_term, print_term};
use serde_json::Value;
use tempfile::tempdir;

fn cmd() -> assert_cmd::Command {
    cargo_bin_cmd!("genesis_parity")
}

fn write_contract_module(dir: &std::path::Path) -> std::path::PathBuf {
    let file = dir.join("m.gc");
    std::fs::write(
        &file,
        r#"
          (def c (core/contract::make (fn (msg) nil) nil {}))
          c
        "#,
    )
    .expect("write module");
    file
}

#[test]
fn debug_frames_and_step_emit_deterministic_trace_payloads() {
    let td = tempdir().unwrap();
    let file = write_contract_module(td.path());

    let out = cmd()
        .args([
            "--json",
            "debug",
            "frames",
            file.to_str().unwrap(),
            "--engine",
            "rust",
            "--contract",
            "c",
            "--msg",
            "(msg foo nil)",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&out).expect("frames json");
    assert_eq!(
        json.get("kind").and_then(Value::as_str),
        Some("genesis/debug-frames-v0.1")
    );
    let trace_hash = json
        .pointer("/data/trace_hash_hex")
        .and_then(Value::as_str)
        .expect("trace hash");
    assert_eq!(trace_hash.len(), 64);
    assert!(trace_hash.chars().all(|ch| ch.is_ascii_hexdigit()));
    let frames = json
        .pointer("/data/frames")
        .and_then(Value::as_str)
        .expect("frames coreform");
    let frames_term = parse_term(frames).expect("parse frames");
    let Term::Vector(frames_vec) = frames_term else {
        panic!("debug frames must return vector");
    };
    assert_eq!(frames_vec.len(), 1);

    let out = cmd()
        .args([
            "--json",
            "debug",
            "step",
            file.to_str().unwrap(),
            "--engine",
            "rust",
            "--contract",
            "c",
            "--msg",
            "(msg foo nil)",
            "--cursor",
            "0",
            "--count",
            "1",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&out).expect("step json");
    assert_eq!(
        json.get("kind").and_then(Value::as_str),
        Some("genesis/debug-step-v0.1")
    );
    assert_eq!(
        json.pointer("/data/selected_step_index")
            .and_then(Value::as_u64),
        Some(0)
    );
    assert_eq!(
        json.pointer("/data/cursor_end").and_then(Value::as_u64),
        Some(1)
    );
}

#[test]
fn debug_break_and_continue_support_key_value_matching() {
    let td = tempdir().unwrap();
    let file = write_contract_module(td.path());

    let inspect_out = cmd()
        .args([
            "--json",
            "debug",
            "inspect",
            file.to_str().unwrap(),
            "--engine",
            "rust",
            "--contract",
            "c",
            "--msg",
            "(msg foo nil)",
            "--index",
            "0",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let inspect_json: Value = serde_json::from_slice(&inspect_out).expect("inspect json");
    let frame = inspect_json
        .pointer("/data/frame")
        .and_then(Value::as_str)
        .expect("frame");
    let frame_term = parse_term(frame).expect("parse frame");
    let Term::Map(frame_map) = frame_term else {
        panic!("frame must be map");
    };
    let k_contract_id = TermOrdKey(Term::symbol(":contract-id"));
    let contract_id = frame_map
        .get(&k_contract_id)
        .and_then(|v| match v {
            Term::Str(s) => Some(s.clone()),
            _ => None,
        })
        .expect("frame :contract-id");
    let match_value = format!("\"{contract_id}\"");

    let break_out = cmd()
        .args([
            "--json",
            "debug",
            "break",
            file.to_str().unwrap(),
            "--engine",
            "rust",
            "--contract",
            "c",
            "--msg",
            "(msg foo nil)",
            "--match-key",
            ":contract-id",
            "--match-value",
            &match_value,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let break_json: Value = serde_json::from_slice(&break_out).expect("break json");
    assert_eq!(
        break_json.pointer("/data/hit").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        break_json
            .pointer("/data/breakpoint_index")
            .and_then(Value::as_u64),
        Some(0)
    );

    let continue_out = cmd()
        .args([
            "--json",
            "debug",
            "continue",
            file.to_str().unwrap(),
            "--engine",
            "rust",
            "--contract",
            "c",
            "--msg",
            "(msg foo nil)",
            "--cursor",
            "0",
            "--match-key",
            ":contract-id",
            "--match-value",
            &match_value,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let continue_json: Value = serde_json::from_slice(&continue_out).expect("continue json");
    assert_eq!(
        continue_json
            .pointer("/data/halted")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        continue_json
            .pointer("/data/halt_reason")
            .and_then(Value::as_str),
        Some("breakpoint")
    );
    assert_eq!(
        continue_json
            .pointer("/data/selected_step_index")
            .and_then(Value::as_u64),
        Some(0)
    );
}

#[test]
fn debug_timeline_and_bisect_detect_first_mismatch() {
    let td = tempdir().unwrap();
    let file = write_contract_module(td.path());
    let baseline = td.path().join("baseline.timeline.gc");
    let candidate = td.path().join("candidate.timeline.gc");

    let timeline_out = cmd()
        .args([
            "--json",
            "debug",
            "timeline",
            file.to_str().unwrap(),
            "--engine",
            "rust",
            "--contract",
            "c",
            "--msg",
            "(msg foo nil)",
            "--out",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let timeline_json: Value = serde_json::from_slice(&timeline_out).expect("timeline json");
    assert_eq!(
        timeline_json.get("kind").and_then(Value::as_str),
        Some("genesis/debug-timeline-v0.1")
    );
    assert!(
        timeline_json
            .pointer("/data/timeline_frame_count")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            >= 1
    );

    let baseline_src = std::fs::read_to_string(&baseline).expect("read baseline timeline");
    let mut baseline_term = parse_term(&baseline_src).expect("parse baseline timeline");
    let Term::Map(root) = &mut baseline_term else {
        panic!("timeline artifact must be map");
    };
    let k_frames = TermOrdKey(Term::symbol(":frames"));
    let Some(Term::Vector(frames)) = root.get_mut(&k_frames) else {
        panic!("timeline artifact missing :frames");
    };
    let Some(Term::Map(first)) = frames.first_mut() else {
        panic!("timeline must contain at least one frame map");
    };
    first.insert(
        TermOrdKey(Term::symbol(":redteam-mutation")),
        Term::Bool(true),
    );
    std::fs::write(&candidate, format!("{}\n", print_term(&baseline_term)))
        .expect("write candidate timeline");

    let bisect_match_out = cmd()
        .args([
            "--json",
            "debug",
            "bisect",
            "--baseline",
            baseline.to_str().unwrap(),
            "--candidate",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let bisect_match_json: Value =
        serde_json::from_slice(&bisect_match_out).expect("bisect match json");
    assert_eq!(
        bisect_match_json.get("kind").and_then(Value::as_str),
        Some("genesis/debug-bisect-v0.1")
    );
    assert_eq!(
        bisect_match_json
            .pointer("/data/match")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert!(
        bisect_match_json
            .pointer("/data/mismatch_index")
            .map(Value::is_null)
            .unwrap_or(false)
    );

    let bisect_out = cmd()
        .args([
            "--json",
            "debug",
            "bisect",
            "--baseline",
            baseline.to_str().unwrap(),
            "--candidate",
            candidate.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let bisect_json: Value = serde_json::from_slice(&bisect_out).expect("bisect json");
    assert_eq!(
        bisect_json.pointer("/data/match").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        bisect_json
            .pointer("/data/mismatch_index")
            .and_then(Value::as_u64),
        Some(0)
    );
}
