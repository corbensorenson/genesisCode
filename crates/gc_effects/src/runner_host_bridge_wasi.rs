use super::*;

fn map_lookup_str_or_sym(
    map: &std::collections::BTreeMap<TermOrdKey, Term>,
    key: &str,
) -> Option<Term> {
    map.get(&TermOrdKey(Term::symbol(key)))
        .or_else(|| map.get(&TermOrdKey(Term::Str(key.to_string()))))
        .cloned()
}

fn wasi_bridge_response_for_op(
    pol: Option<&OpPolicy>,
    op: &str,
    family: &str,
    max_bytes: Option<usize>,
) -> Result<Option<Term>, BridgeError> {
    let Some(pol) = pol else {
        return Ok(None);
    };

    if let Some(raw) = pol
        .extra
        .get("wasi_bridge_response")
        .and_then(|v| v.as_str())
    {
        let parsed = parse_term(raw).map_err(|e| BridgeError {
            code: "wasi/bridge-response-parse".to_string(),
            message: format!("wasi_bridge_response parse error: {e}"),
        })?;
        return Ok(Some(parsed));
    }

    if let Some(raw) = pol
        .extra
        .get("wasi_bridge_responses")
        .and_then(|v| v.as_str())
    {
        let parsed = parse_term(raw).map_err(|e| BridgeError {
            code: "wasi/bridge-responses-parse".to_string(),
            message: format!("wasi_bridge_responses parse error: {e}"),
        })?;
        if let Term::Map(m) = parsed
            && let Some(resp) = map_lookup_str_or_sym(&m, op)
        {
            return Ok(Some(resp));
        }
    }

    if let Some(file_raw) = pol
        .extra
        .get("wasi_bridge_response_file")
        .and_then(|v| v.as_str())
    {
        let base_dir = effective_base_dir(Some(pol)).map_err(|e| BridgeError {
            code: "wasi/bridge-response-file-path".to_string(),
            message: e.to_string(),
        })?;
        // Portable decimal u64 framing header plus its newline; policy bounds the
        // complete document body, including an aggregate op-response map.
        let wire_limit = max_bytes
            .map(|limit| {
                limit
                    .checked_add(u64::MAX.to_string().len() + 1)
                    .ok_or_else(|| BridgeError {
                        code: format!("{family}/bridge-policy"),
                        message: "max_bytes leaves no room for response framing".to_owned(),
                    })
            })
            .transpose()?;
        let file = sandbox_document_read(&base_dir, file_raw).map_err(|e| BridgeError {
            code: "wasi/bridge-response-file-path".to_string(),
            message: e.to_string(),
        })?;
        let bytes = file.read_bytes(wire_limit).map_err(|e| match e {
            FsReadError::LimitExceeded { .. } => BridgeError {
                code: format!("{family}/bridge-response-too-large"),
                message: "bridge profile document exceeds max_bytes and framing allowance"
                    .to_owned(),
            },
            FsReadError::Io(error) => BridgeError {
                code: "wasi/bridge-response-file-read".to_string(),
                message: error.to_string(),
            },
            FsReadError::Cancelled => BridgeError {
                code: "wasi/bridge-response-file-read".to_string(),
                message: "document read cancelled".to_owned(),
            },
        })?;
        if let Some(limit) = max_bytes {
            let body_len = std::str::from_utf8(&bytes)
                .ok()
                .and_then(|text| text.split_once('\n'))
                .and_then(|(header, body)| {
                    header
                        .trim()
                        .parse::<usize>()
                        .ok()
                        .filter(|length| *length == body.len())
                })
                .unwrap_or(bytes.len());
            if body_len > limit {
                return Err(BridgeError {
                    code: format!("{family}/bridge-response-too-large"),
                    message: format!(
                        "bridge profile document exceeds max_bytes ({body_len} > {limit})"
                    ),
                });
            }
        }
        let parsed = decode_bridge_stdout("wasi", &bytes, None)?;
        if let Term::Map(ref m) = parsed
            && let Some(resp) = map_lookup_str_or_sym(m, op)
        {
            return Ok(Some(resp));
        }
        return Ok(Some(parsed));
    }

    if let Ok(raw) = std::env::var("GENESIS_WASI_BRIDGE_RESPONSES") {
        let parsed = parse_term(&raw).map_err(|e| BridgeError {
            code: "wasi/bridge-env-parse".to_string(),
            message: e.to_string(),
        })?;
        if let Term::Map(m) = parsed
            && let Some(resp) = map_lookup_str_or_sym(&m, op)
        {
            return Ok(Some(resp));
        }
    }

    Ok(None)
}

pub(crate) fn run_wasi_bridge_profile(
    family: &str,
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    max_bytes: Option<usize>,
) -> Result<Term, BridgeError> {
    runner_host_bridge_policy::enforce_payload_limit(family, payload, max_bytes)?;
    let Some(response) = wasi_bridge_response_for_op(pol, op, family, max_bytes)? else {
        return Err(BridgeError {
            code: format!("{family}/bridge-wasi-profile-required"),
            message: format!(
                "{op} requires wasi bridge profile data (set per-op `wasi_bridge_response`/`wasi_bridge_response_file` or GENESIS_WASI_BRIDGE_RESPONSES)"
            ),
        });
    };
    runner_host_bridge_policy::enforce_response_limit(family, &response, max_bytes)?;
    Ok(response)
}

#[cfg(test)]
mod document_read_controls {
    use super::*;

    #[test]
    fn profile_document_accepts_exact_body_and_framing_and_preserves_selection() {
        let fixture = tempfile::tempdir().unwrap();
        let policy = crate::CapsPolicy::from_toml_str(&format!(
            "allow = [\"gpu/compute::limits\"]\n[op.\"gpu/compute::limits\"]\nbase_dir = {:?}\nwasi_bridge_profile = true\nwasi_bridge_response_file = \"response.gc\"\n",
            fixture.path().to_str().unwrap()
        )).unwrap();
        let operation = "gpu/compute::limits";
        for body in [
            "{:ok true}",
            "{gpu/compute::limits {:ok true} other/op {:ok false}}",
        ] {
            for framed in [false, true] {
                let wire = if framed {
                    format!("{}\n{body}", body.len())
                } else {
                    body.to_owned()
                };
                std::fs::write(fixture.path().join("response.gc"), wire).unwrap();
                assert_eq!(
                    run_wasi_bridge_profile(
                        "gpu",
                        operation,
                        &Term::Nil,
                        policy.op_policy(operation),
                        Some(body.len())
                    )
                    .unwrap(),
                    parse_term("{:ok true}").unwrap()
                );
                assert_eq!(
                    run_wasi_bridge_profile(
                        "gpu",
                        operation,
                        &Term::Nil,
                        policy.op_policy(operation),
                        Some(body.len() - 1)
                    )
                    .unwrap_err()
                    .code,
                    "gpu/bridge-response-too-large"
                );
            }
        }
        std::fs::write(
            fixture.path().join("response.gc"),
            "                   {:ok true}",
        )
        .unwrap();
        assert_eq!(
            run_wasi_bridge_profile(
                "gpu",
                operation,
                &Term::Nil,
                policy.op_policy(operation),
                Some(10)
            )
            .unwrap_err()
            .code,
            "gpu/bridge-response-too-large"
        );
    }

    #[test]
    fn profile_file_bounds_transport_before_parsing() {
        let fixture = tempfile::tempdir().unwrap();
        let mut bytes = vec![b' '; 1024 * 1024];
        bytes.extend_from_slice(b"{:ok true}");
        std::fs::write(fixture.path().join("response.gc"), bytes).unwrap();
        let policy = crate::CapsPolicy::from_toml_str(&format!(
            "allow = [\"gpu/compute::limits\"]\n[op.\"gpu/compute::limits\"]\nbase_dir = {:?}\nwasi_bridge_profile = true\nwasi_bridge_response_file = \"response.gc\"\nmax_bytes = 32\n",
            fixture.path().to_str().unwrap()
        )).unwrap();
        let error = run_wasi_bridge_profile(
            "gpu",
            "gpu/compute::limits",
            &Term::Nil,
            policy.op_policy("gpu/compute::limits"),
            Some(32),
        )
        .unwrap_err();
        assert_eq!(error.code, "gpu/bridge-response-too-large");
    }

    #[cfg(unix)]
    #[test]
    fn profile_document_keeps_admitted_file_after_ancestor_replacement() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("root");
        let outside = fixture.path().join("outside");
        std::fs::create_dir_all(root.join("responses")).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(root.join("responses/response.gc"), "{:inside true}").unwrap();
        std::fs::write(outside.join("response.gc"), "{:outside true}").unwrap();
        let admitted = sandbox_document_read(&root, "responses/response.gc").unwrap();
        std::fs::rename(root.join("responses"), root.join("retained")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("responses")).unwrap();
        let parsed =
            decode_bridge_stdout("wasi", &admitted.read_bytes(Some(64)).unwrap(), None).unwrap();
        assert_eq!(parsed, parse_term("{:inside true}").unwrap());
    }
}
