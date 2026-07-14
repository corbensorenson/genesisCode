use super::*;

#[test]
fn roots_reject_escape_and_non_file_schemes() {
    let root = std::env::current_dir()
        .expect("cwd")
        .canonicalize()
        .expect("canonical");
    let config = Config {
        max_queue: 1,
        max_frame_bytes: 1024,
        max_output_bytes: 1024,
        max_requests: 10,
        max_roots: 2,
        workspace_boundary: root.clone(),
    };
    assert!(validate_roots(&[json!({"uri": "https://example.com"})], &config).is_err());
    let parent = root.parent().expect("parent");
    let uri = file_uri(parent).expect("uri");
    assert!(validate_roots(&[json!({"uri": uri})], &config).is_err());
}

#[test]
fn multiple_roots_require_an_explicit_exact_uri() {
    let cwd = std::env::current_dir().expect("cwd");
    let state = State {
        roots: BTreeMap::from([
            ("file:///a".to_string(), cwd.clone()),
            ("file:///b".to_string(), cwd),
        ]),
        ..State::new(Path::new("."))
    };
    assert!(select_root(None, &state).is_err());
    assert!(select_root(Some(&json!("file:///missing")), &state).is_err());
}

#[test]
fn cancellation_removes_queued_calls_and_suppresses_running_calls() {
    let cwd = std::env::current_dir().expect("cwd");
    let cli = Cli::try_parse_from(["genesis", "mcp"]).expect("CLI");
    let mut state = State::new(&cwd);
    state.active_ids.insert("\"queued\"".to_string());
    state.pending.push_back(PendingCall {
        id: json!("queued"),
        key: "\"queued\"".to_string(),
        progress_token: Some(json!("progress")),
        cli,
        workspace_root: cwd,
    });
    cancel_request(&json!({"requestId": "queued"}), &mut state);
    assert!(state.pending.is_empty());
    assert!(!state.active_ids.contains("\"queued\""));

    state.running = Some(RunningCall {
        id: json!(7),
        key: "7".to_string(),
        progress_token: None,
        cancelled: false,
    });
    cancel_request(&json!({"requestId": 7}), &mut state);
    assert!(state.running.as_ref().is_some_and(|call| call.cancelled));
}
