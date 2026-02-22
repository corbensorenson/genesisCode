use super::*;

#[test]
fn io_db_connect_policy_gate_enforces_target_allowlist() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["io/db::connect"]

[op."io/db::connect"]
db_target_allow = ["sqlite://data/app.db"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :connection-id \"db-1\"}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = term_map([(
        Term::symbol(":target"),
        Term::Str("sqlite://data/other.db".to_string()),
    )]);
    let out = call_capability(
        "io/db::connect",
        &payload,
        policy.op_policy("io/db::connect"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(62),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn io_db_query_policy_requires_query_class_allowlist_and_limits() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["io/db::query"]

[op."io/db::query"]
allow_query_classes = ["read-only"]
max_row_count = 500
max_result_bytes = 8192
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :rows [{:id 1 :name \"alice\"}] :row-count 1}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let denied = call_capability(
        "io/db::query",
        &term_map([
            (
                Term::symbol(":connection-id"),
                Term::Str("db-1".to_string()),
            ),
            (
                Term::symbol(":query-class"),
                Term::Symbol("write".to_string()),
            ),
            (
                Term::symbol(":query"),
                Term::Str("update users set name='x'".to_string()),
            ),
        ]),
        policy.op_policy("io/db::query"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(63),
    )
    .expect("query");
    assert_eq!(code_from_error(denied), "core/caps/policy-error");

    let allowed = call_capability(
        "io/db::query",
        &term_map([
            (
                Term::symbol(":connection-id"),
                Term::Str("db-1".to_string()),
            ),
            (
                Term::symbol(":query-class"),
                Term::Symbol("read-only".to_string()),
            ),
            (
                Term::symbol(":query"),
                Term::Str("select id, name from users".to_string()),
            ),
        ]),
        policy.op_policy("io/db::query"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(64),
    )
    .expect("query");
    let Value::Data(Term::Map(mm)) = allowed else {
        panic!("expected query data map");
    };
    assert_eq!(
        mm.get(&TermOrdKey(Term::symbol(":row-count"))),
        Some(&Term::Int(1_i64.into()))
    );
}

#[test]
fn io_db_sql_and_kv_family_wasi_bridge_profile_returns_data() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = [
  "io/db::connect",
  "io/db::tx-begin",
  "io/db::query",
  "io/db::exec",
  "io/db::tx-commit",
  "io/db::tx-rollback",
  "io/db::kv-open",
  "io/db::kv-get",
  "io/db::kv-put",
  "io/db::kv-delete"
]

[op."io/db::connect"]
db_target_allow = ["sqlite://data/app.db"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :connection-id \"db-1\"}"

[op."io/db::tx-begin"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :tx-id \"tx-1\"}"

[op."io/db::query"]
allow_query_classes = ["read-only", "write"]
max_row_count = 500
max_result_bytes = 8192
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :rows [{:id 1}] :row-count 1}"

[op."io/db::exec"]
allow_query_classes = ["write"]
max_result_bytes = 4096
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :affected-rows 1}"

[op."io/db::tx-commit"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :committed true}"

[op."io/db::tx-rollback"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :rolled-back true}"

[op."io/db::kv-open"]
db_target_allow = ["kv://state/main"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :store-id \"kv-1\"}"

[op."io/db::kv-get"]
max_result_bytes = 4096
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :found true :value \"v1\"}"

[op."io/db::kv-put"]
max_value_bytes = 4096
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :written true}"

[op."io/db::kv-delete"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :deleted true}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();

    let connect = call_capability(
        "io/db::connect",
        &term_map([(
            Term::symbol(":target"),
            Term::Str("sqlite://data/app.db".to_string()),
        )]),
        policy.op_policy("io/db::connect"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(65),
    )
    .expect("connect");
    let Value::Data(Term::Map(connect_mm)) = connect else {
        panic!("expected connect map");
    };
    assert_eq!(
        connect_mm.get(&TermOrdKey(Term::symbol(":connection-id"))),
        Some(&Term::Str("db-1".to_string()))
    );

    let begin = call_capability(
        "io/db::tx-begin",
        &term_map([(
            Term::symbol(":connection-id"),
            Term::Str("db-1".to_string()),
        )]),
        policy.op_policy("io/db::tx-begin"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(66),
    )
    .expect("tx-begin");
    let Value::Data(Term::Map(begin_mm)) = begin else {
        panic!("expected tx-begin map");
    };
    assert_eq!(
        begin_mm.get(&TermOrdKey(Term::symbol(":tx-id"))),
        Some(&Term::Str("tx-1".to_string()))
    );

    let exec = call_capability(
        "io/db::exec",
        &term_map([
            (
                Term::symbol(":connection-id"),
                Term::Str("db-1".to_string()),
            ),
            (
                Term::symbol(":query-class"),
                Term::Symbol("write".to_string()),
            ),
            (
                Term::symbol(":statement"),
                Term::Str("update users set name='bob' where id=1".to_string()),
            ),
        ]),
        policy.op_policy("io/db::exec"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(67),
    )
    .expect("exec");
    let Value::Data(Term::Map(exec_mm)) = exec else {
        panic!("expected exec map");
    };
    assert_eq!(
        exec_mm.get(&TermOrdKey(Term::symbol(":affected-rows"))),
        Some(&Term::Int(1_i64.into()))
    );

    let kv_open = call_capability(
        "io/db::kv-open",
        &term_map([(
            Term::symbol(":target"),
            Term::Str("kv://state/main".to_string()),
        )]),
        policy.op_policy("io/db::kv-open"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(68),
    )
    .expect("kv-open");
    let Value::Data(Term::Map(kv_open_mm)) = kv_open else {
        panic!("expected kv-open map");
    };
    assert_eq!(
        kv_open_mm.get(&TermOrdKey(Term::symbol(":store-id"))),
        Some(&Term::Str("kv-1".to_string()))
    );

    let kv_get = call_capability(
        "io/db::kv-get",
        &term_map([
            (Term::symbol(":store-id"), Term::Str("kv-1".to_string())),
            (Term::symbol(":key"), Term::Str("alpha".to_string())),
        ]),
        policy.op_policy("io/db::kv-get"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(69),
    )
    .expect("kv-get");
    let Value::Data(Term::Map(kv_get_mm)) = kv_get else {
        panic!("expected kv-get map");
    };
    assert_eq!(
        kv_get_mm.get(&TermOrdKey(Term::symbol(":found"))),
        Some(&Term::Bool(true))
    );
}

#[test]
fn sys_process_exec_policy_gate_requires_allowlisted_program() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["sys/process::exec"]

[op."sys/process::exec"]
allow_programs = ["gcpm"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :status 0}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = Term::Map(
        [(
            TermOrdKey(Term::symbol(":program")),
            Term::Str("bash".to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let out = call_capability(
        "sys/process::exec",
        &payload,
        policy.op_policy("sys/process::exec"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(11),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn sys_process_exec_wasi_bridge_profile_returns_data() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["sys/process::exec"]

[op."sys/process::exec"]
allow_programs = ["gcpm"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :status 0 :stdout \"ready\"}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = Term::Map(
        [(
            TermOrdKey(Term::symbol(":program")),
            Term::Str("gcpm".to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let out = call_capability(
        "sys/process::exec",
        &payload,
        policy.op_policy("sys/process::exec"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(13),
    )
    .expect("call capability");
    let Value::Data(Term::Map(mm)) = out else {
        panic!("expected data map");
    };
    assert_eq!(
        mm.get(&TermOrdKey(Term::symbol(":status"))),
        Some(&Term::Int(0_i64.into()))
    );
}

#[test]
fn io_fs_extended_ops_execute_with_deterministic_payload_contracts() {
    let temp = tempdir().expect("tempdir");
    let base_dir = temp.path().display().to_string().replace('\\', "/");
    let policy = CapsPolicy::from_toml_str(&format!(
        r#"
allow = ["io/fs::mkdir", "io/fs::write", "io/fs::stat", "io/fs::list", "io/fs::rename", "io/fs::remove"]

[op."io/fs::mkdir"]
base_dir = "{base_dir}"

[op."io/fs::write"]
base_dir = "{base_dir}"
create_dirs = true

[op."io/fs::stat"]
base_dir = "{base_dir}"

[op."io/fs::list"]
base_dir = "{base_dir}"

[op."io/fs::rename"]
base_dir = "{base_dir}"
create_dirs = true

[op."io/fs::remove"]
base_dir = "{base_dir}"
"#
    ))
    .expect("caps");

    let mut budget = ArtifactBudgetState::default();
    let mkdir_payload = term_map([
        (Term::symbol(":path"), Term::Str("sandbox/work".to_string())),
        (Term::symbol(":parents"), Term::Bool(true)),
    ]);
    let mkdir_out = call_capability(
        "io/fs::mkdir",
        &mkdir_payload,
        policy.op_policy("io/fs::mkdir"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(70),
    )
    .expect("io/fs::mkdir");
    assert!(matches!(mkdir_out, Value::Data(Term::Nil)));

    let write_payload = term_map([
        (
            Term::symbol(":path"),
            Term::Str("sandbox/work/input.txt".to_string()),
        ),
        (Term::symbol(":data"), Term::Bytes(b"hello".to_vec().into())),
    ]);
    let write_out = call_capability(
        "io/fs::write",
        &write_payload,
        policy.op_policy("io/fs::write"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(71),
    )
    .expect("io/fs::write");
    assert!(matches!(write_out, Value::Data(Term::Nil)));

    let stat_payload = term_map([(
        Term::symbol(":path"),
        Term::Str("sandbox/work/input.txt".to_string()),
    )]);
    let stat_out = call_capability(
        "io/fs::stat",
        &stat_payload,
        policy.op_policy("io/fs::stat"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(72),
    )
    .expect("io/fs::stat");
    let Value::Data(Term::Map(stat_map)) = stat_out else {
        panic!("expected stat data map");
    };
    assert_eq!(
        stat_map.get(&TermOrdKey(Term::symbol(":exists"))),
        Some(&Term::Bool(true))
    );
    assert_eq!(
        stat_map.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(&Term::Symbol("file".to_string()))
    );

    let list_payload = term_map([(Term::symbol(":path"), Term::Str("sandbox/work".to_string()))]);
    let list_out = call_capability(
        "io/fs::list",
        &list_payload,
        policy.op_policy("io/fs::list"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(73),
    )
    .expect("io/fs::list");
    let Value::Data(Term::Vector(entries)) = list_out else {
        panic!("expected list vector");
    };
    assert!(entries.iter().any(|entry| {
        let Term::Map(mm) = entry else {
            return false;
        };
        mm.get(&TermOrdKey(Term::symbol(":name"))) == Some(&Term::Str("input.txt".to_string()))
    }));

    let rename_payload = term_map([
        (
            Term::symbol(":from"),
            Term::Str("sandbox/work/input.txt".to_string()),
        ),
        (
            Term::symbol(":to"),
            Term::Str("sandbox/work/output.txt".to_string()),
        ),
    ]);
    let rename_out = call_capability(
        "io/fs::rename",
        &rename_payload,
        policy.op_policy("io/fs::rename"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(74),
    )
    .expect("io/fs::rename");
    assert!(matches!(rename_out, Value::Data(Term::Nil)));

    let remove_payload = term_map([(
        Term::symbol(":path"),
        Term::Str("sandbox/work/output.txt".to_string()),
    )]);
    let remove_out = call_capability(
        "io/fs::remove",
        &remove_payload,
        policy.op_policy("io/fs::remove"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(75),
    )
    .expect("io/fs::remove");
    assert!(matches!(remove_out, Value::Data(Term::Nil)));

    let stat_missing_out = call_capability(
        "io/fs::stat",
        &term_map([(
            Term::symbol(":path"),
            Term::Str("sandbox/work/output.txt".to_string()),
        )]),
        policy.op_policy("io/fs::stat"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(76),
    )
    .expect("io/fs::stat missing");
    let Value::Data(Term::Map(missing_map)) = stat_missing_out else {
        panic!("expected stat data map");
    };
    assert_eq!(
        missing_map.get(&TermOrdKey(Term::symbol(":exists"))),
        Some(&Term::Bool(false))
    );
}

#[test]
fn io_fs_mutating_ops_reject_timeout_policy() {
    let temp = tempdir().expect("tempdir");
    let base_dir = temp.path().display().to_string().replace('\\', "/");
    let policy = CapsPolicy::from_toml_str(&format!(
        r#"
allow = ["io/fs::mkdir"]

[op."io/fs::mkdir"]
base_dir = "{base_dir}"
timeout_ms = 5
"#
    ))
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let out = call_capability(
        "io/fs::mkdir",
        &term_map([(Term::symbol(":path"), Term::Str("sandbox/work".to_string()))]),
        policy.op_policy("io/fs::mkdir"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(77),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn core_media_asset_hash_and_transcode_ops_are_deterministic_and_policy_gated() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["core/media::asset-hash", "core/media::image-transcode", "core/media::audio-transcode"]

[op."core/media::asset-hash"]
max_input_bytes = 16

[op."core/media::image-transcode"]
max_input_bytes = 16
max_output_bytes = 16
max_pixels = 16
allow_source_formats = ["rgba8", "gray8"]
allow_target_formats = ["rgba8", "gray8"]

[op."core/media::audio-transcode"]
max_input_bytes = 16
max_output_bytes = 32
max_frames = 16
min_sample_rate = 8000
max_sample_rate = 96000
allow_source_formats = ["pcm-s16le", "pcm-f32le"]
allow_target_formats = ["pcm-s16le", "pcm-f32le"]
"#,
    )
    .expect("caps");

    let mut budget = ArtifactBudgetState::default();

    let hash_out = call_capability(
        "core/media::asset-hash",
        &term_map([(Term::symbol(":data"), Term::Bytes(b"hello".to_vec().into()))]),
        policy.op_policy("core/media::asset-hash"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(200),
    )
    .expect("hash op");
    let Value::Data(Term::Map(hash_map)) = hash_out else {
        panic!("expected hash map");
    };
    assert_eq!(
        hash_map.get(&TermOrdKey(Term::symbol(":algorithm"))),
        Some(&Term::Str("blake3".to_string()))
    );
    assert_eq!(
        hash_map.get(&TermOrdKey(Term::symbol(":bytes"))),
        Some(&Term::Int(5_i64.into()))
    );

    let image_payload = term_map([
        (
            Term::symbol(":data"),
            Term::Bytes(vec![0, 128, 255, 255, 255, 0, 0, 255].into()),
        ),
        (
            Term::symbol(":source-format"),
            Term::Str("rgba8".to_string()),
        ),
        (
            Term::symbol(":target-format"),
            Term::Str("gray8".to_string()),
        ),
        (Term::symbol(":width"), Term::Int(2_i64.into())),
        (Term::symbol(":height"), Term::Int(1_i64.into())),
    ]);
    let image_out = call_capability(
        "core/media::image-transcode",
        &image_payload,
        policy.op_policy("core/media::image-transcode"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(201),
    )
    .expect("image transcode");
    let Value::Data(Term::Map(image_map)) = image_out else {
        panic!("expected image map");
    };
    assert_eq!(
        image_map.get(&TermOrdKey(Term::symbol(":output-bytes"))),
        Some(&Term::Int(2_i64.into()))
    );
    let Some(Term::Bytes(image_bytes)) = image_map.get(&TermOrdKey(Term::symbol(":data"))) else {
        panic!("expected image output :data bytes");
    };
    assert_eq!(image_bytes.as_ref(), &[104, 77]);

    let audio_payload = term_map([
        (
            Term::symbol(":data"),
            Term::Bytes(vec![0, 0, 255, 127, 0, 128, 16, 0].into()),
        ),
        (
            Term::symbol(":source-format"),
            Term::Str("pcm-s16le".to_string()),
        ),
        (
            Term::symbol(":target-format"),
            Term::Str("pcm-f32le".to_string()),
        ),
        (Term::symbol(":channels"), Term::Int(1_i64.into())),
        (Term::symbol(":sample-rate"), Term::Int(44100_i64.into())),
    ]);
    let audio_out = call_capability(
        "core/media::audio-transcode",
        &audio_payload,
        policy.op_policy("core/media::audio-transcode"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(202),
    )
    .expect("audio transcode");
    let Value::Data(Term::Map(audio_map)) = audio_out else {
        panic!("expected audio map");
    };
    assert_eq!(
        audio_map.get(&TermOrdKey(Term::symbol(":frames"))),
        Some(&Term::Int(4_i64.into()))
    );
    assert_eq!(
        audio_map.get(&TermOrdKey(Term::symbol(":output-bytes"))),
        Some(&Term::Int(16_i64.into()))
    );
}

#[test]
fn core_media_transcode_rejects_disallowed_format() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["core/media::image-transcode"]

[op."core/media::image-transcode"]
allow_source_formats = ["gray8"]
allow_target_formats = ["gray8"]
max_pixels = 16
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let out = call_capability(
        "core/media::image-transcode",
        &term_map([
            (
                Term::symbol(":data"),
                Term::Bytes(vec![0, 0, 0, 255].into()),
            ),
            (
                Term::symbol(":source-format"),
                Term::Str("rgba8".to_string()),
            ),
            (
                Term::symbol(":target-format"),
                Term::Str("gray8".to_string()),
            ),
            (Term::symbol(":width"), Term::Int(1_i64.into())),
            (Term::symbol(":height"), Term::Int(1_i64.into())),
        ]),
        policy.op_policy("core/media::image-transcode"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(203),
    )
    .expect("image transcode");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn sys_process_spawn_and_stream_ops_use_bridge_contracts() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = [
  "sys/process::spawn",
  "sys/process::wait",
  "sys/process::kill",
  "sys/process::stdin-write",
  "sys/process::stdout-read",
  "sys/process::stderr-read"
]

[op."sys/process::spawn"]
allow_programs = ["gcpm"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :process-id \"proc-1\"}"

[op."sys/process::wait"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :status 0}"

[op."sys/process::kill"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :killed true}"

[op."sys/process::stdin-write"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :written-bytes 4}"

[op."sys/process::stdout-read"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :data b\"done\" :eof true}"

[op."sys/process::stderr-read"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :data b\"\" :eof true}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();

    let spawn_out = call_capability(
        "sys/process::spawn",
        &term_map([(Term::symbol(":program"), Term::Str("gcpm".to_string()))]),
        policy.op_policy("sys/process::spawn"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(78),
    )
    .expect("spawn");
    let Value::Data(Term::Map(spawn_map)) = spawn_out else {
        panic!("expected spawn map");
    };
    let Some(Term::Str(process_id)) = spawn_map.get(&TermOrdKey(Term::symbol(":process-id")))
    else {
        panic!("missing process-id");
    };
    assert_eq!(process_id, "proc-1");

    let wait_payload = term_map([(Term::symbol(":process-id"), Term::Str(process_id.clone()))]);
    let wait_out = call_capability(
        "sys/process::wait",
        &wait_payload,
        policy.op_policy("sys/process::wait"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(79),
    )
    .expect("wait");
    let Value::Data(Term::Map(wait_map)) = wait_out else {
        panic!("expected wait map");
    };
    assert_eq!(
        wait_map.get(&TermOrdKey(Term::symbol(":status"))),
        Some(&Term::Int(0_i64.into()))
    );

    let write_out = call_capability(
        "sys/process::stdin-write",
        &term_map([
            (Term::symbol(":process-id"), Term::Str(process_id.clone())),
            (Term::symbol(":data"), Term::Bytes(b"ping".to_vec().into())),
        ]),
        policy.op_policy("sys/process::stdin-write"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(80),
    )
    .expect("stdin-write");
    let Value::Data(Term::Map(write_map)) = write_out else {
        panic!("expected stdin-write map");
    };
    assert_eq!(
        write_map.get(&TermOrdKey(Term::symbol(":written-bytes"))),
        Some(&Term::Int(4_i64.into()))
    );

    let stdout_out = call_capability(
        "sys/process::stdout-read",
        &wait_payload,
        policy.op_policy("sys/process::stdout-read"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(81),
    )
    .expect("stdout-read");
    let Value::Data(Term::Map(stdout_map)) = stdout_out else {
        panic!("expected stdout-read map");
    };
    assert_eq!(
        stdout_map.get(&TermOrdKey(Term::symbol(":eof"))),
        Some(&Term::Bool(true))
    );

    let stderr_out = call_capability(
        "sys/process::stderr-read",
        &wait_payload,
        policy.op_policy("sys/process::stderr-read"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(82),
    )
    .expect("stderr-read");
    let Value::Data(Term::Map(stderr_map)) = stderr_out else {
        panic!("expected stderr-read map");
    };
    assert_eq!(
        stderr_map.get(&TermOrdKey(Term::symbol(":eof"))),
        Some(&Term::Bool(true))
    );

    let kill_out = call_capability(
        "sys/process::kill",
        &wait_payload,
        policy.op_policy("sys/process::kill"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(83),
    )
    .expect("kill");
    let Value::Data(Term::Map(kill_map)) = kill_out else {
        panic!("expected kill map");
    };
    assert_eq!(
        kill_map.get(&TermOrdKey(Term::symbol(":killed"))),
        Some(&Term::Bool(true))
    );
}

#[test]
fn host_plugin_policy_gate_requires_allowlisted_plugin() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/plugin::command"]

[op."host/plugin::command"]
allow_plugins = ["demo"]
allow_commands = ["run"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":plugin")),
                Term::Str("other".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":command")),
                Term::Str("run".to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let out = call_capability(
        "host/plugin::command",
        &payload,
        policy.op_policy("host/plugin::command"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(17),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn host_plugin_policy_gate_requires_command_allowlist() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/plugin::command"]

[op."host/plugin::command"]
allow_plugins = ["demo"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":plugin")),
                Term::Str("demo".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":command")),
                Term::Str("run".to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let out = call_capability(
        "host/plugin::command",
        &payload,
        policy.op_policy("host/plugin::command"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(18),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn host_plugin_bridge_transport_requires_digest_pin() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/plugin::command"]

[op."host/plugin::command"]
allow_plugins = ["demo"]
allow_commands = ["run"]
base_dir = "."
bridge_cmd = "bridge.sh"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":plugin")),
                Term::Str("demo".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":command")),
                Term::Str("run".to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let out = call_capability(
        "host/plugin::command",
        &payload,
        policy.op_policy("host/plugin::command"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(19),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn host_plugin_wasi_bridge_profile_returns_data() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/plugin::command"]

[op."host/plugin::command"]
allow_plugins = ["demo"]
allow_commands = ["run"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :status \"ok\"}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":plugin")),
                Term::Str("demo".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":command")),
                Term::Symbol("run".to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let out = call_capability(
        "host/plugin::command",
        &payload,
        policy.op_policy("host/plugin::command"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(19),
    )
    .expect("call capability");
    let Value::Data(Term::Map(mm)) = out else {
        panic!("expected data map");
    };
    assert_eq!(
        mm.get(&TermOrdKey(Term::symbol(":status"))),
        Some(&Term::Str("ok".to_string()))
    );
}

#[test]
fn host_plugin_typed_schema_requires_schema_allowlist() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/plugin::command"]

[op."host/plugin::command"]
allow_plugins = ["demo"]
allow_commands = ["run"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :result {:exit-code 0}}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = term_map([
        (Term::symbol(":plugin"), Term::Str("demo".to_string())),
        (Term::symbol(":command"), Term::Str("run".to_string())),
        (
            Term::symbol(":request-schema-id"),
            Term::Str("genesis/plugin.request.exec.v1".to_string()),
        ),
        (
            Term::symbol(":response-schema-id"),
            Term::Str("genesis/plugin.response.result.v1".to_string()),
        ),
        (
            Term::symbol(":payload"),
            term_map([(Term::symbol(":args"), Term::Vector(vec![]))]),
        ),
    ]);
    let out = call_capability(
        "host/plugin::command",
        &payload,
        policy.op_policy("host/plugin::command"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(20),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn host_plugin_typed_schema_rejects_bad_request_payload() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/plugin::command"]

[op."host/plugin::command"]
allow_plugins = ["demo"]
allow_commands = ["run"]
allow_schema_ids = ["genesis/plugin.request.exec.v1", "genesis/plugin.response.result.v1"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :result {:exit-code 0}}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = term_map([
        (Term::symbol(":plugin"), Term::Str("demo".to_string())),
        (Term::symbol(":command"), Term::Str("run".to_string())),
        (
            Term::symbol(":request-schema"),
            Term::Str("genesis/plugin.request.exec.v1".to_string()),
        ),
        (
            Term::symbol(":response-schema"),
            Term::Str("genesis/plugin.response.result.v1".to_string()),
        ),
        (
            Term::symbol(":payload"),
            term_map([(Term::symbol(":method"), Term::Str("run".to_string()))]),
        ),
    ]);
    let out = call_capability(
        "host/plugin::command",
        &payload,
        policy.op_policy("host/plugin::command"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(21),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/schema-error");
}

#[test]
fn host_plugin_typed_schema_rejects_bad_response_payload() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/plugin::command"]

[op."host/plugin::command"]
allow_plugins = ["demo"]
allow_commands = ["run"]
allow_schema_ids = ["genesis/plugin.request.exec.v1", "genesis/plugin.response.result.v1"]
wasi_bridge_profile = true
wasi_bridge_response = "{:status \"ok\"}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = term_map([
        (Term::symbol(":plugin"), Term::Str("demo".to_string())),
        (Term::symbol(":command"), Term::Str("run".to_string())),
        (
            Term::symbol(":request-schema-id"),
            Term::Str("genesis/plugin.request.exec.v1".to_string()),
        ),
        (
            Term::symbol(":response-schema-id"),
            Term::Str("genesis/plugin.response.result.v1".to_string()),
        ),
        (
            Term::symbol(":payload"),
            term_map([(Term::symbol(":args"), Term::Vector(vec![]))]),
        ),
    ]);
    let out = call_capability(
        "host/plugin::command",
        &payload,
        policy.op_policy("host/plugin::command"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(22),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/schema-error");
}

#[test]
fn host_plugin_typed_schema_accepts_valid_request_and_response() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/plugin::command"]

[op."host/plugin::command"]
allow_plugins = ["demo"]
allow_commands = ["run"]
allow_schema_ids = ["genesis/plugin.request.exec.v1", "genesis/plugin.response.result.v1"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :result {:exit-code 0}}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = term_map([
        (Term::symbol(":plugin"), Term::Str("demo".to_string())),
        (Term::symbol(":command"), Term::Str("run".to_string())),
        (
            Term::symbol(":request-schema-id"),
            Term::Str("genesis/plugin.request.exec.v1".to_string()),
        ),
        (
            Term::symbol(":response-schema-id"),
            Term::Str("genesis/plugin.response.result.v1".to_string()),
        ),
        (
            Term::symbol(":payload"),
            term_map([
                (
                    Term::symbol(":args"),
                    Term::Vector(vec![Term::Str("--help".to_string())]),
                ),
                (Term::symbol(":cwd"), Term::Str("/tmp".to_string())),
            ]),
        ),
    ]);
    let out = call_capability(
        "host/plugin::command",
        &payload,
        policy.op_policy("host/plugin::command"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(23),
    )
    .expect("call capability");
    let Value::Data(Term::Map(mm)) = out else {
        panic!("expected data map");
    };
    assert_eq!(
        mm.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(&Term::Bool(true))
    );
}
