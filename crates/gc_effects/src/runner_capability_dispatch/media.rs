use super::*;
#[path = "media_formats.rs"]
mod media_formats;
use media_formats::{
    audio_bytes_per_sample, audio_supported_formats, audio_transcode, image_bytes_per_pixel,
    image_supported_formats, image_transcode,
};

fn payload_error_value(error_tok: SealId, op: &str, err: EffectsError) -> Value {
    mk_error(
        error_tok,
        "core/caps/payload-error",
        err.to_string(),
        Some(op),
    )
}

fn payload_required_positive_usize_field(
    payload: &Term,
    op: &str,
    key: &str,
) -> Result<usize, EffectsError> {
    let value = payload_required_field(payload, op, key)?;
    let Term::Int(n) = value else {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload field `{key}` must be an int"
        )));
    };
    let Some(as_i64) = n.to_i64() else {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload field `{key}` must fit i64"
        )));
    };
    if as_i64 <= 0 {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload field `{key}` must be > 0"
        )));
    }
    usize::try_from(as_i64).map_err(|_| {
        EffectsError::BadPayload(format!(
            "{op} payload field `{key}` exceeds platform usize range"
        ))
    })
}

fn media_policy_allowlist(
    pol: Option<&OpPolicy>,
    key: &str,
    default: &[&str],
) -> Result<Vec<String>, String> {
    let Some(pol) = pol else {
        return Ok(default.iter().map(|x| x.to_string()).collect());
    };
    let Some(v) = pol.extra.get(key) else {
        return Ok(default.iter().map(|x| x.to_string()).collect());
    };
    let Some(arr) = v.as_array() else {
        return Err(format!("{key} must be an array of strings"));
    };
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let normalized = match item {
            toml::Value::String(s) => s.trim().to_string(),
            _ => {
                return Err(format!("{key} entries must be strings"));
            }
        };
        if !normalized.is_empty() {
            out.push(normalized.to_ascii_lowercase());
        }
    }
    if out.is_empty() {
        return Err(format!("{key} must contain at least one format"));
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn media_policy_positive_usize(
    pol: Option<&OpPolicy>,
    key: &str,
    default_value: usize,
) -> Result<usize, String> {
    match op_extra_positive_usize(pol, key)? {
        Some(value) => Ok(value),
        None => Ok(default_value),
    }
}

fn media_policy_contains(allowlist: &[String], value: &str) -> bool {
    allowlist.iter().any(|allowed| allowed == value)
}

fn media_hash_response(op: &str, data: &[u8], kind: Option<&str>, algorithm: &str) -> Value {
    let mut response = BTreeMap::new();
    response.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
    response.insert(TermOrdKey(Term::symbol(":op")), Term::Str(op.to_string()));
    response.insert(
        TermOrdKey(Term::symbol(":algorithm")),
        Term::Str(algorithm.to_string()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":hash")),
        Term::Str(blake3::hash(data).to_hex().to_string()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":bytes")),
        Term::Int((data.len() as i64).into()),
    );
    if let Some(kind) = kind {
        response.insert(
            TermOrdKey(Term::symbol(":kind")),
            Term::Str(kind.to_string()),
        );
    }
    Value::data(Term::Map(response))
}

pub(super) fn capability_core_media_asset_hash(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let data = match payload_data(payload) {
        Ok(bytes) => bytes,
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };
    let max_input_bytes =
        match media_policy_positive_usize(pol, "max_input_bytes", 16 * 1024 * 1024) {
            Ok(v) => v,
            Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
        };
    if data.len() > max_input_bytes {
        return Ok(mk_resource_limit_error(
            error_tok,
            op,
            "media input bytes",
            data.len(),
            max_input_bytes,
        ));
    }
    let algorithm = match payload_optional_field(payload, op, ":algorithm") {
        Ok(Some(Term::Str(s) | Term::Symbol(s))) => s.trim().to_ascii_lowercase(),
        Ok(Some(_)) => {
            return Ok(mk_error(
                error_tok,
                "core/caps/payload-error",
                format!("{op} payload field `:algorithm` must be string/symbol"),
                Some(op),
            ));
        }
        Ok(None) => "blake3".to_string(),
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };
    if algorithm != "blake3" {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} only supports algorithm `blake3`"),
            Some(op),
        ));
    }
    let kind = match payload_optional_field(payload, op, ":kind") {
        Ok(Some(Term::Str(s) | Term::Symbol(s))) => Some(s.trim().to_ascii_lowercase()),
        Ok(Some(_)) => {
            return Ok(mk_error(
                error_tok,
                "core/caps/payload-error",
                format!("{op} payload field `:kind` must be string/symbol"),
                Some(op),
            ));
        }
        Ok(None) => None,
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };
    Ok(media_hash_response(op, &data, kind.as_deref(), &algorithm))
}

pub(super) fn capability_core_media_image_transcode(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let data = match payload_data(payload) {
        Ok(bytes) => bytes,
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };
    let source_format = match payload_required_string_or_symbol_field(payload, op, ":source-format")
    {
        Ok(value) => value.to_ascii_lowercase(),
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };
    let target_format = match payload_required_string_or_symbol_field(payload, op, ":target-format")
    {
        Ok(value) => value.to_ascii_lowercase(),
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };
    let width = match payload_required_positive_usize_field(payload, op, ":width") {
        Ok(value) => value,
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };
    let height = match payload_required_positive_usize_field(payload, op, ":height") {
        Ok(value) => value,
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };

    let max_input_bytes =
        match media_policy_positive_usize(pol, "max_input_bytes", 16 * 1024 * 1024) {
            Ok(v) => v,
            Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
        };
    if data.len() > max_input_bytes {
        return Ok(mk_resource_limit_error(
            error_tok,
            op,
            "media input bytes",
            data.len(),
            max_input_bytes,
        ));
    }

    let max_output_bytes =
        match media_policy_positive_usize(pol, "max_output_bytes", 32 * 1024 * 1024) {
            Ok(v) => v,
            Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
        };
    let max_pixels = match media_policy_positive_usize(pol, "max_pixels", 8_388_608) {
        Ok(v) => v,
        Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
    };
    let allow_source =
        match media_policy_allowlist(pol, "allow_source_formats", image_supported_formats()) {
            Ok(v) => v,
            Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
        };
    let allow_target =
        match media_policy_allowlist(pol, "allow_target_formats", image_supported_formats()) {
            Ok(v) => v,
            Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
        };
    if !media_policy_contains(&allow_source, &source_format) {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} source format `{source_format}` is not allowlisted"),
            Some(op),
        ));
    }
    if !media_policy_contains(&allow_target, &target_format) {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} target format `{target_format}` is not allowlisted"),
            Some(op),
        ));
    }

    let Some(pixel_count) = width.checked_mul(height) else {
        return Ok(mk_error(
            error_tok,
            "core/caps/resource-limit",
            format!("{op} pixel count overflow for width={width} height={height}"),
            Some(op),
        ));
    };
    if pixel_count > max_pixels {
        return Ok(mk_error(
            error_tok,
            "core/caps/resource-limit",
            format!("{op} pixel count exceeds policy limit ({pixel_count} > {max_pixels})"),
            Some(op),
        ));
    }

    let Some(source_bpp) = image_bytes_per_pixel(&source_format) else {
        return Ok(mk_error(
            error_tok,
            "core/caps/payload-error",
            format!("{op} unsupported source format `{source_format}`"),
            Some(op),
        ));
    };
    let Some(target_bpp) = image_bytes_per_pixel(&target_format) else {
        return Ok(mk_error(
            error_tok,
            "core/caps/payload-error",
            format!("{op} unsupported target format `{target_format}`"),
            Some(op),
        ));
    };

    let Some(expected_input_len) = pixel_count.checked_mul(source_bpp) else {
        return Ok(mk_error(
            error_tok,
            "core/caps/resource-limit",
            format!("{op} input length overflow"),
            Some(op),
        ));
    };
    if data.len() != expected_input_len {
        return Ok(mk_error(
            error_tok,
            "core/caps/payload-error",
            format!(
                "{op} input bytes mismatch: expected {expected_input_len}, got {}",
                data.len()
            ),
            Some(op),
        ));
    }

    let mut output = match image_transcode(&source_format, &target_format, width, height, &data) {
        Ok(transcoded) => transcoded,
        Err(detail) => {
            let code = if detail.contains("overflow") {
                "core/caps/resource-limit"
            } else {
                "core/caps/payload-error"
            };
            return Ok(mk_error(
                error_tok,
                code,
                format!("{op} {detail}"),
                Some(op),
            ));
        }
    };

    let Some(expected_output_len) = pixel_count.checked_mul(target_bpp) else {
        return Ok(mk_error(
            error_tok,
            "core/caps/resource-limit",
            format!("{op} output length overflow"),
            Some(op),
        ));
    };
    if output.len() != expected_output_len {
        return Ok(mk_error(
            error_tok,
            "core/caps/resource-limit",
            format!(
                "{op} output byte mismatch: expected {expected_output_len}, got {}",
                output.len()
            ),
            Some(op),
        ));
    }
    if output.len() > max_output_bytes {
        return Ok(mk_resource_limit_error(
            error_tok,
            op,
            "media output bytes",
            output.len(),
            max_output_bytes,
        ));
    }

    let mut response = BTreeMap::new();
    response.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
    response.insert(
        TermOrdKey(Term::symbol(":kind")),
        Term::Str("image".to_string()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":source-format")),
        Term::Str(source_format),
    );
    response.insert(
        TermOrdKey(Term::symbol(":target-format")),
        Term::Str(target_format),
    );
    response.insert(
        TermOrdKey(Term::symbol(":width")),
        Term::Int((width as i64).into()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":height")),
        Term::Int((height as i64).into()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":input-bytes")),
        Term::Int((data.len() as i64).into()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":output-bytes")),
        Term::Int((output.len() as i64).into()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":hash")),
        Term::Str(blake3::hash(&output).to_hex().to_string()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":data")),
        Term::Bytes(std::mem::take(&mut output).into()),
    );
    Ok(Value::data(Term::Map(response)))
}

pub(super) fn capability_core_media_audio_transcode(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let data = match payload_data(payload) {
        Ok(bytes) => bytes,
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };
    let source_format = match payload_required_string_or_symbol_field(payload, op, ":source-format")
    {
        Ok(value) => value.to_ascii_lowercase(),
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };
    let target_format = match payload_required_string_or_symbol_field(payload, op, ":target-format")
    {
        Ok(value) => value.to_ascii_lowercase(),
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };
    let channels = match payload_required_positive_usize_field(payload, op, ":channels") {
        Ok(v) => v,
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };
    let sample_rate = match payload_required_positive_usize_field(payload, op, ":sample-rate") {
        Ok(v) => v,
        Err(err) => return Ok(payload_error_value(error_tok, op, err)),
    };

    let max_input_bytes =
        match media_policy_positive_usize(pol, "max_input_bytes", 16 * 1024 * 1024) {
            Ok(v) => v,
            Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
        };
    if data.len() > max_input_bytes {
        return Ok(mk_resource_limit_error(
            error_tok,
            op,
            "media input bytes",
            data.len(),
            max_input_bytes,
        ));
    }
    if channels > 8 {
        return Ok(mk_error(
            error_tok,
            "core/caps/resource-limit",
            format!("{op} channels exceeds limit ({} > 8)", channels),
            Some(op),
        ));
    }

    let max_frames = match media_policy_positive_usize(pol, "max_frames", 2_000_000) {
        Ok(v) => v,
        Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
    };
    let max_output_bytes =
        match media_policy_positive_usize(pol, "max_output_bytes", 32 * 1024 * 1024) {
            Ok(v) => v,
            Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
        };
    let min_sample_rate = match media_policy_positive_usize(pol, "min_sample_rate", 8_000) {
        Ok(v) => v,
        Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
    };
    let max_sample_rate = match media_policy_positive_usize(pol, "max_sample_rate", 192_000) {
        Ok(v) => v,
        Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
    };
    if sample_rate < min_sample_rate || sample_rate > max_sample_rate {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!(
                "{op} sample-rate {} outside policy bounds [{}, {}]",
                sample_rate, min_sample_rate, max_sample_rate
            ),
            Some(op),
        ));
    }

    let allow_source =
        match media_policy_allowlist(pol, "allow_source_formats", audio_supported_formats()) {
            Ok(v) => v,
            Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
        };
    let allow_target =
        match media_policy_allowlist(pol, "allow_target_formats", audio_supported_formats()) {
            Ok(v) => v,
            Err(e) => return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op))),
        };
    if !media_policy_contains(&allow_source, &source_format) {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} source format `{source_format}` is not allowlisted"),
            Some(op),
        ));
    }
    if !media_policy_contains(&allow_target, &target_format) {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!("{op} target format `{target_format}` is not allowlisted"),
            Some(op),
        ));
    }

    let Some(source_sample_bytes) = audio_bytes_per_sample(&source_format) else {
        return Ok(mk_error(
            error_tok,
            "core/caps/payload-error",
            format!("{op} unsupported source format `{source_format}`"),
            Some(op),
        ));
    };
    let Some(target_sample_bytes) = audio_bytes_per_sample(&target_format) else {
        return Ok(mk_error(
            error_tok,
            "core/caps/payload-error",
            format!("{op} unsupported target format `{target_format}`"),
            Some(op),
        ));
    };

    let Some(input_frame_bytes) = source_sample_bytes.checked_mul(channels) else {
        return Ok(mk_error(
            error_tok,
            "core/caps/resource-limit",
            format!("{op} input frame-size overflow"),
            Some(op),
        ));
    };
    if data.len() % input_frame_bytes != 0 {
        return Ok(mk_error(
            error_tok,
            "core/caps/payload-error",
            format!(
                "{op} input bytes ({}) not aligned to frame size {}",
                data.len(),
                input_frame_bytes
            ),
            Some(op),
        ));
    }
    let expected_frames = data.len() / input_frame_bytes;
    if expected_frames > max_frames {
        return Ok(mk_error(
            error_tok,
            "core/caps/resource-limit",
            format!("{op} frame count exceeds policy limit ({expected_frames} > {max_frames})"),
            Some(op),
        ));
    }

    let (output, frames) = match audio_transcode(&source_format, &target_format, channels, &data) {
        Ok(result) => result,
        Err(detail) => {
            let code = if detail.contains("overflow") {
                "core/caps/resource-limit"
            } else {
                "core/caps/payload-error"
            };
            return Ok(mk_error(
                error_tok,
                code,
                format!("{op} {detail}"),
                Some(op),
            ));
        }
    };

    let Some(expected_output) = frames
        .checked_mul(channels)
        .and_then(|n| n.checked_mul(target_sample_bytes))
    else {
        return Ok(mk_error(
            error_tok,
            "core/caps/resource-limit",
            format!("{op} output frame-size overflow"),
            Some(op),
        ));
    };
    if output.len() != expected_output {
        return Ok(mk_error(
            error_tok,
            "core/caps/resource-limit",
            format!(
                "{op} output bytes mismatch: expected {expected_output}, got {}",
                output.len()
            ),
            Some(op),
        ));
    }
    if output.len() > max_output_bytes {
        return Ok(mk_resource_limit_error(
            error_tok,
            op,
            "media output bytes",
            output.len(),
            max_output_bytes,
        ));
    }

    let mut response = BTreeMap::new();
    response.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
    response.insert(
        TermOrdKey(Term::symbol(":kind")),
        Term::Str("audio".to_string()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":source-format")),
        Term::Str(source_format),
    );
    response.insert(
        TermOrdKey(Term::symbol(":target-format")),
        Term::Str(target_format),
    );
    response.insert(
        TermOrdKey(Term::symbol(":channels")),
        Term::Int((channels as i64).into()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":sample-rate")),
        Term::Int((sample_rate as i64).into()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":frames")),
        Term::Int((frames as i64).into()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":input-bytes")),
        Term::Int((data.len() as i64).into()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":output-bytes")),
        Term::Int((output.len() as i64).into()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":hash")),
        Term::Str(blake3::hash(&output).to_hex().to_string()),
    );
    response.insert(
        TermOrdKey(Term::symbol(":data")),
        Term::Bytes(output.into()),
    );
    Ok(Value::data(Term::Map(response)))
}
