use super::*;

pub(super) fn first_party_layer_create(
    runtime: &mut XrHostRuntime,
    payload: &Term,
    pol: Option<&OpPolicy>,
) -> Term {
    let op = "gfx/xr::layer-create";
    let session = match required_session(runtime, payload, op) {
        Ok(session) => session,
        Err(err) => return err,
    };
    let max_layers = match pol {
        Some(pol) => match parse_positive_policy_i64(pol, "max_layers", 16, op) {
            Ok(value) => value,
            Err(err) => return err,
        },
        None => 16,
    };
    if session.layers.len() as i64 >= max_layers {
        return policy_error(op, "layer capacity exceeded max_layers policy");
    }
    let allow_layer_types = match parse_policy_string_allowlist(
        pol,
        "allow_layer_types",
        &["quad", "cylinder", "equirect"],
        op,
    ) {
        Ok(values) => values,
        Err(err) => return err,
    };
    let layer_type = parse_optional_string(payload, ":type")
        .unwrap_or_else(|| "quad".to_string())
        .trim()
        .to_ascii_lowercase();
    if !allow_layer_types
        .iter()
        .any(|allowed| allowed == &layer_type)
    {
        return policy_error(op, &format!("layer type `{layer_type}` is not allowlisted"));
    }
    let layout = parse_optional_string(payload, ":layout")
        .unwrap_or_else(|| "stereo".to_string())
        .trim()
        .to_ascii_lowercase();
    if layout.is_empty() {
        return bad_payload_error(op, "payload field `:layout` must not be empty");
    }
    let max_layer_opacity = match pol {
        Some(pol) => match parse_positive_policy_i64(pol, "max_layer_opacity", 1000, op) {
            Ok(value) => value,
            Err(err) => return err,
        },
        None => 1000,
    };
    let opacity = parse_optional_positive_i64(payload, ":opacity")
        .unwrap_or(1000)
        .min(max_layer_opacity);
    let transform = parse_optional_term(payload, ":transform").unwrap_or_else(default_pose);

    session.layer_seq = session.layer_seq.saturating_add(1);
    let layer_id = format!("xr-layer-{}", session.layer_seq);
    session.layers.insert(
        layer_id.clone(),
        XrLayerState {
            layer_type: layer_type.clone(),
            layout: layout.clone(),
            opacity,
            transform: transform.clone(),
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
        (":layer-id", Term::Str(layer_id)),
        (":type", Term::Str(layer_type)),
        (":layout", Term::Str(layout)),
        (":opacity", Term::Int(opacity.into())),
        (":transform", transform),
        (
            ":layer-count",
            Term::Int((session.layers.len() as i64).into()),
        ),
    ])
}

pub(super) fn first_party_layer_update(
    runtime: &mut XrHostRuntime,
    payload: &Term,
    pol: Option<&OpPolicy>,
) -> Term {
    let op = "gfx/xr::layer-update";
    let sid = match payload_session_id(payload) {
        Some(sid) => sid,
        None => return missing_session_error(op),
    };
    let session = match required_session(runtime, payload, op) {
        Ok(session) => session,
        Err(err) => return err,
    };
    let layer_id = match parse_optional_string(payload, ":layer-id") {
        Some(id) if !id.trim().is_empty() => id.trim().to_string(),
        _ => return bad_payload_error(op, "payload field `:layer-id` is required"),
    };
    let Some(layer) = session.layers.get_mut(&layer_id) else {
        return unknown_layer_error(op, &sid, &layer_id);
    };
    if let Some(layout) = parse_optional_string(payload, ":layout") {
        let normalized = layout.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return bad_payload_error(op, "payload field `:layout` must not be empty");
        }
        layer.layout = normalized;
    }
    if let Some(layer_type) = parse_optional_string(payload, ":type") {
        let normalized = layer_type.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return bad_payload_error(op, "payload field `:type` must not be empty");
        }
        layer.layer_type = normalized;
    }
    if let Some(opacity) = parse_optional_positive_i64(payload, ":opacity") {
        let max_layer_opacity = match pol {
            Some(pol) => match parse_positive_policy_i64(pol, "max_layer_opacity", 1000, op) {
                Ok(value) => value,
                Err(err) => return err,
            },
            None => 1000,
        };
        if opacity > max_layer_opacity {
            return policy_error(
                op,
                &format!(
                    "layer opacity {} exceeds max_layer_opacity {}",
                    opacity, max_layer_opacity
                ),
            );
        }
        layer.opacity = opacity;
    }
    if let Some(transform) = parse_optional_term(payload, ":transform") {
        layer.transform = transform;
    }
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(XR_FIRST_PARTY_BACKEND.to_string())),
        (":adapter", Term::Str(XR_FIRST_PARTY_ADAPTER.to_string())),
        (":session-id", Term::Str(sid)),
        (":layer-id", Term::Str(layer_id)),
        (":type", Term::Str(layer.layer_type.clone())),
        (":layout", Term::Str(layer.layout.clone())),
        (":opacity", Term::Int(layer.opacity.into())),
        (":transform", layer.transform.clone()),
    ])
}

pub(super) fn first_party_layer_destroy(runtime: &mut XrHostRuntime, payload: &Term) -> Term {
    let op = "gfx/xr::layer-destroy";
    let sid = match payload_session_id(payload) {
        Some(sid) => sid,
        None => return missing_session_error(op),
    };
    let session = match required_session(runtime, payload, op) {
        Ok(session) => session,
        Err(err) => return err,
    };
    let layer_id = match parse_optional_string(payload, ":layer-id") {
        Some(id) if !id.trim().is_empty() => id.trim().to_string(),
        _ => return bad_payload_error(op, "payload field `:layer-id` is required"),
    };
    if session.layers.remove(&layer_id).is_none() {
        return unknown_layer_error(op, &sid, &layer_id);
    }
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(XR_FIRST_PARTY_BACKEND.to_string())),
        (":adapter", Term::Str(XR_FIRST_PARTY_ADAPTER.to_string())),
        (":session-id", Term::Str(sid)),
        (":layer-id", Term::Str(layer_id)),
        (":destroyed", Term::Bool(true)),
        (
            ":layer-count",
            Term::Int((session.layers.len() as i64).into()),
        ),
    ])
}
