use super::*;

#[path = "advanced_layers.rs"]
mod advanced_layers;

fn parse_policy_bool(pol: Option<&OpPolicy>, key: &str, default_value: bool) -> Result<bool, Term> {
    let Some(pol) = pol else {
        return Ok(default_value);
    };
    let Some(v) = pol.extra.get(key) else {
        return Ok(default_value);
    };
    let Some(value) = v.as_bool() else {
        return Err(policy_error(
            "gfx/xr::policy",
            &format!("`{key}` must be a boolean"),
        ));
    };
    Ok(value)
}

fn parse_policy_string_allowlist(
    pol: Option<&OpPolicy>,
    key: &str,
    defaults: &[&str],
    op: &str,
) -> Result<Vec<String>, Term> {
    let Some(pol) = pol else {
        return Ok(defaults.iter().map(|x| x.to_string()).collect());
    };
    let Some(v) = pol.extra.get(key) else {
        return Ok(defaults.iter().map(|x| x.to_string()).collect());
    };
    let Some(arr) = v.as_array() else {
        return Err(policy_error(
            op,
            &format!("`{key}` must be an array of strings"),
        ));
    };
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let Some(raw) = item.as_str() else {
            return Err(policy_error(
                op,
                &format!("`{key}` entries must be strings"),
            ));
        };
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_ascii_lowercase());
        }
    }
    if out.is_empty() {
        return Err(policy_error(
            op,
            &format!("`{key}` must contain at least one entry"),
        ));
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn parse_optional_positive_i64(payload: &Term, key: &str) -> Option<i64> {
    payload_map(payload)
        .and_then(|m| map_get_i64(m, key))
        .filter(|v| *v > 0)
}

fn parse_optional_string(payload: &Term, key: &str) -> Option<String> {
    payload_map(payload).and_then(|m| map_get_string(m, key))
}

fn parse_optional_term(payload: &Term, key: &str) -> Option<Term> {
    payload_map(payload)
        .and_then(|m| m.get(&TermOrdKey(Term::symbol(key))))
        .cloned()
}

fn required_session<'a>(
    runtime: &'a mut XrHostRuntime,
    payload: &Term,
    op: &str,
) -> Result<&'a mut XrSessionState, Term> {
    let Some(session_id) = payload_session_id(payload) else {
        return Err(missing_session_error(op));
    };
    let Some(session) = runtime.sessions.get_mut(&session_id) else {
        return Err(unknown_session_error(op, &session_id));
    };
    if !session.open {
        return Err(closed_session_error(op, &session_id));
    }
    Ok(session)
}

fn default_pose() -> Term {
    map_term(vec![
        (
            ":position",
            Term::Vector(vec![
                Term::Int(0_i64.into()),
                Term::Int(0_i64.into()),
                Term::Int(0_i64.into()),
            ]),
        ),
        (
            ":orientation",
            Term::Vector(vec![
                Term::Int(0_i64.into()),
                Term::Int(0_i64.into()),
                Term::Int(0_i64.into()),
                Term::Int(1_i64.into()),
            ]),
        ),
    ])
}

fn unknown_anchor_error(op: &str, sid: &str, anchor_id: &str) -> Term {
    map_term(vec![
        (":ok", Term::Bool(false)),
        (
            ":error/code",
            Term::Str("gfx/xr-first-party-unknown-anchor".to_string()),
        ),
        (":error/op", Term::symbol(op)),
        (":session-id", Term::Str(sid.to_string())),
        (":anchor-id", Term::Str(anchor_id.to_string())),
    ])
}

fn unknown_layer_error(op: &str, sid: &str, layer_id: &str) -> Term {
    map_term(vec![
        (":ok", Term::Bool(false)),
        (
            ":error/code",
            Term::Str("gfx/xr-first-party-unknown-layer".to_string()),
        ),
        (":error/op", Term::symbol(op)),
        (":session-id", Term::Str(sid.to_string())),
        (":layer-id", Term::Str(layer_id.to_string())),
    ])
}

pub(super) fn first_party_layer_create(
    runtime: &mut XrHostRuntime,
    payload: &Term,
    pol: Option<&OpPolicy>,
) -> Term {
    advanced_layers::first_party_layer_create(runtime, payload, pol)
}

pub(super) fn first_party_layer_update(
    runtime: &mut XrHostRuntime,
    payload: &Term,
    pol: Option<&OpPolicy>,
) -> Term {
    advanced_layers::first_party_layer_update(runtime, payload, pol)
}

pub(super) fn first_party_layer_destroy(runtime: &mut XrHostRuntime, payload: &Term) -> Term {
    advanced_layers::first_party_layer_destroy(runtime, payload)
}

pub(super) fn first_party_anchor_create(
    runtime: &mut XrHostRuntime,
    payload: &Term,
    pol: Option<&OpPolicy>,
) -> Term {
    let op = "gfx/xr::anchor-create";
    let session = match required_session(runtime, payload, op) {
        Ok(session) => session,
        Err(err) => return err,
    };
    let max_anchors = match pol {
        Some(pol) => match parse_positive_policy_i64(pol, "max_anchors", 64, op) {
            Ok(value) => value,
            Err(err) => return err,
        },
        None => 64,
    };
    if session.anchors.len() as i64 >= max_anchors {
        return policy_error(op, "anchor capacity exceeded max_anchors policy");
    }

    let space = parse_optional_string(payload, ":space")
        .unwrap_or_else(|| session.reference_space.clone())
        .trim()
        .to_ascii_lowercase();
    if space.is_empty() {
        return bad_payload_error(op, "payload field `:space` must not be empty");
    }
    let allow_spaces = match parse_policy_string_allowlist(
        pol,
        "allow_anchor_spaces",
        &["local", "local-floor", "bounded-floor", "viewer"],
        op,
    ) {
        Ok(allow) => allow,
        Err(err) => return err,
    };
    if !allow_spaces.iter().any(|allowed| allowed == &space) {
        return policy_error(op, &format!("anchor space `{space}` is not allowlisted"));
    }

    let pose = parse_optional_term(payload, ":pose").unwrap_or_else(default_pose);
    let label = parse_optional_string(payload, ":label")
        .unwrap_or_else(|| format!("anchor-{}", session.anchor_seq.saturating_add(1)));
    session.anchor_seq = session.anchor_seq.saturating_add(1);
    let anchor_id = format!("xr-anchor-{}", session.anchor_seq);
    session.anchors.insert(
        anchor_id.clone(),
        XrAnchorState {
            space: space.clone(),
            label: label.clone(),
            pose: pose.clone(),
        },
    );
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(XR_FIRST_PARTY_BACKEND.to_string())),
        (":adapter", Term::Str(XR_FIRST_PARTY_ADAPTER.to_string())),
        (
            ":session-id",
            Term::Str(payload_session_id(payload).unwrap_or_default()),
        ),
        (":anchor-id", Term::Str(anchor_id)),
        (":space", Term::Str(space)),
        (":label", Term::Str(label)),
        (":pose", pose),
        (":tracking-state", Term::symbol(":tracked")),
        (
            ":anchor-count",
            Term::Int((session.anchors.len() as i64).into()),
        ),
    ])
}

pub(super) fn first_party_anchor_update(
    runtime: &mut XrHostRuntime,
    payload: &Term,
    _pol: Option<&OpPolicy>,
) -> Term {
    let op = "gfx/xr::anchor-update";
    let sid = match payload_session_id(payload) {
        Some(sid) => sid,
        None => return missing_session_error(op),
    };
    let session = match required_session(runtime, payload, op) {
        Ok(session) => session,
        Err(err) => return err,
    };
    let anchor_id = match parse_optional_string(payload, ":anchor-id") {
        Some(id) if !id.trim().is_empty() => id.trim().to_string(),
        _ => return bad_payload_error(op, "payload field `:anchor-id` is required"),
    };
    let Some(anchor) = session.anchors.get_mut(&anchor_id) else {
        return unknown_anchor_error(op, &sid, &anchor_id);
    };
    if let Some(space) = parse_optional_string(payload, ":space") {
        let normalized = space.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return bad_payload_error(op, "payload field `:space` must not be empty");
        }
        anchor.space = normalized;
    }
    if let Some(label) = parse_optional_string(payload, ":label") {
        let normalized = label.trim().to_string();
        if normalized.is_empty() {
            return bad_payload_error(op, "payload field `:label` must not be empty");
        }
        anchor.label = normalized;
    }
    if let Some(pose) = parse_optional_term(payload, ":pose") {
        anchor.pose = pose;
    }
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(XR_FIRST_PARTY_BACKEND.to_string())),
        (":adapter", Term::Str(XR_FIRST_PARTY_ADAPTER.to_string())),
        (":session-id", Term::Str(sid)),
        (":anchor-id", Term::Str(anchor_id)),
        (":space", Term::Str(anchor.space.clone())),
        (":label", Term::Str(anchor.label.clone())),
        (":pose", anchor.pose.clone()),
        (":tracking-state", Term::symbol(":tracked")),
    ])
}

pub(super) fn first_party_anchor_destroy(runtime: &mut XrHostRuntime, payload: &Term) -> Term {
    let op = "gfx/xr::anchor-destroy";
    let sid = match payload_session_id(payload) {
        Some(sid) => sid,
        None => return missing_session_error(op),
    };
    let session = match required_session(runtime, payload, op) {
        Ok(session) => session,
        Err(err) => return err,
    };
    let anchor_id = match parse_optional_string(payload, ":anchor-id") {
        Some(id) if !id.trim().is_empty() => id.trim().to_string(),
        _ => return bad_payload_error(op, "payload field `:anchor-id` is required"),
    };
    if session.anchors.remove(&anchor_id).is_none() {
        return unknown_anchor_error(op, &sid, &anchor_id);
    }
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(XR_FIRST_PARTY_BACKEND.to_string())),
        (":adapter", Term::Str(XR_FIRST_PARTY_ADAPTER.to_string())),
        (":session-id", Term::Str(sid)),
        (":anchor-id", Term::Str(anchor_id)),
        (":destroyed", Term::Bool(true)),
        (
            ":anchor-count",
            Term::Int((session.anchors.len() as i64).into()),
        ),
    ])
}

pub(super) fn first_party_hands_poll(
    runtime: &mut XrHostRuntime,
    payload: &Term,
    pol: Option<&OpPolicy>,
) -> Term {
    let op = "gfx/xr::hands-poll";
    let session = match required_session(runtime, payload, op) {
        Ok(session) => session,
        Err(err) => return err,
    };
    let allow_hand_tracking = match parse_policy_bool(pol, "allow_hand_tracking", true) {
        Ok(flag) => flag,
        Err(err) => return err,
    };
    if !allow_hand_tracking {
        return policy_error(op, "hand tracking disabled by allow_hand_tracking policy");
    }
    let max_hand_joints = match pol {
        Some(pol) => match parse_positive_policy_i64(pol, "max_hand_joints", 25, op) {
            Ok(value) => value,
            Err(err) => return err,
        },
        None => 25,
    };
    let requested_joints = parse_optional_positive_i64(payload, ":max-joints").unwrap_or(25);
    let joints_count = requested_joints.min(max_hand_joints).max(1) as usize;
    let hands = vec![(":left", 0_i64), (":right", 1_i64)]
        .into_iter()
        .map(|(hand, bias)| {
            let joints = (0..joints_count)
                .map(|idx| {
                    let i = idx as i64;
                    map_term(vec![
                        (":joint-index", Term::Int(i.into())),
                        (
                            ":position",
                            Term::Vector(vec![
                                Term::Int((bias + i).into()),
                                Term::Int((10 + i).into()),
                                Term::Int((20 + i).into()),
                            ]),
                        ),
                        (":tracking-state", Term::symbol(":tracked")),
                    ])
                })
                .collect::<Vec<_>>();
            map_term(vec![
                (":hand", Term::symbol(hand)),
                (":joints", Term::Vector(joints)),
            ])
        })
        .collect::<Vec<_>>();
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(XR_FIRST_PARTY_BACKEND.to_string())),
        (":adapter", Term::Str(XR_FIRST_PARTY_ADAPTER.to_string())),
        (
            ":session-id",
            Term::Str(payload_session_id(payload).unwrap_or_default()),
        ),
        (":hands", Term::Vector(hands)),
        (":mode", Term::Str(session.mode.clone())),
    ])
}

pub(super) fn first_party_hit_test(
    runtime: &mut XrHostRuntime,
    payload: &Term,
    pol: Option<&OpPolicy>,
) -> Term {
    let op = "gfx/xr::hit-test";
    let session = match required_session(runtime, payload, op) {
        Ok(session) => session,
        Err(err) => return err,
    };
    let allow_hit_test = match parse_policy_bool(pol, "allow_hit_test", true) {
        Ok(flag) => flag,
        Err(err) => return err,
    };
    if !allow_hit_test {
        return policy_error(op, "hit test disabled by allow_hit_test policy");
    }
    let max_hit_results = match pol {
        Some(pol) => match parse_positive_policy_i64(pol, "max_hit_results", 8, op) {
            Ok(value) => value,
            Err(err) => return err,
        },
        None => 8,
    };
    let requested_hits = parse_optional_positive_i64(payload, ":max-hits").unwrap_or(1);
    let hit_count = requested_hits.min(max_hit_results).max(1) as usize;
    let hits = (0..hit_count)
        .map(|idx| {
            let i = idx as i64;
            map_term(vec![
                (":hit-id", Term::Str(format!("xr-hit-{}", i + 1))),
                (":distance-mm", Term::Int((1200 + i * 160).into())),
                (":space", Term::Str(session.reference_space.clone())),
                (
                    ":pose",
                    map_term(vec![
                        (
                            ":position",
                            Term::Vector(vec![
                                Term::Int((i + 1).into()),
                                Term::Int((i + 2).into()),
                                Term::Int((i + 3).into()),
                            ]),
                        ),
                        (
                            ":normal",
                            Term::Vector(vec![
                                Term::Int(0_i64.into()),
                                Term::Int(1_i64.into()),
                                Term::Int(0_i64.into()),
                            ]),
                        ),
                    ]),
                ),
            ])
        })
        .collect::<Vec<_>>();
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(XR_FIRST_PARTY_BACKEND.to_string())),
        (":adapter", Term::Str(XR_FIRST_PARTY_ADAPTER.to_string())),
        (
            ":session-id",
            Term::Str(payload_session_id(payload).unwrap_or_default()),
        ),
        (":hits", Term::Vector(hits)),
    ])
}

pub(super) fn first_party_spatial_mesh_poll(
    runtime: &mut XrHostRuntime,
    payload: &Term,
    pol: Option<&OpPolicy>,
) -> Term {
    let op = "gfx/xr::spatial-mesh-poll";
    let _session = match required_session(runtime, payload, op) {
        Ok(session) => session,
        Err(err) => return err,
    };
    let allow_mesh = match parse_policy_bool(pol, "allow_spatial_mesh", true) {
        Ok(flag) => flag,
        Err(err) => return err,
    };
    if !allow_mesh {
        return policy_error(
            op,
            "spatial mesh polling disabled by allow_spatial_mesh policy",
        );
    }
    let max_meshes = match pol {
        Some(pol) => match parse_positive_policy_i64(pol, "max_meshes", 4, op) {
            Ok(value) => value,
            Err(err) => return err,
        },
        None => 4,
    };
    let max_vertices = match pol {
        Some(pol) => match parse_positive_policy_i64(pol, "max_mesh_vertices", 4096, op) {
            Ok(value) => value,
            Err(err) => return err,
        },
        None => 4096,
    };
    let requested_meshes = parse_optional_positive_i64(payload, ":max-meshes").unwrap_or(1);
    let mesh_count = requested_meshes.min(max_meshes).max(1) as usize;
    let lod = parse_optional_string(payload, ":lod")
        .unwrap_or_else(|| "medium".to_string())
        .trim()
        .to_ascii_lowercase();
    let meshes = (0..mesh_count)
        .map(|idx| {
            let i = idx as i64;
            let vertex_count = (256 + i * 64).min(max_vertices).max(16);
            let triangle_count = (vertex_count / 3).max(8);
            map_term(vec![
                (":mesh-id", Term::Str(format!("xr-mesh-{}", i + 1))),
                (":lod", Term::Str(lod.clone())),
                (":vertex-count", Term::Int(vertex_count.into())),
                (":triangle-count", Term::Int(triangle_count.into())),
                (
                    ":bounds",
                    map_term(vec![
                        (
                            ":center",
                            Term::Vector(vec![
                                Term::Int(i.into()),
                                Term::Int(0_i64.into()),
                                Term::Int((10 + i).into()),
                            ]),
                        ),
                        (
                            ":extents",
                            Term::Vector(vec![
                                Term::Int(2_i64.into()),
                                Term::Int(2_i64.into()),
                                Term::Int(2_i64.into()),
                            ]),
                        ),
                    ]),
                ),
            ])
        })
        .collect::<Vec<_>>();
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(XR_FIRST_PARTY_BACKEND.to_string())),
        (":adapter", Term::Str(XR_FIRST_PARTY_ADAPTER.to_string())),
        (
            ":session-id",
            Term::Str(payload_session_id(payload).unwrap_or_default()),
        ),
        (":meshes", Term::Vector(meshes)),
    ])
}
