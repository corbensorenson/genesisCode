use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HumanRenderOptions {
    pub(crate) width: usize,
    pub(crate) color: bool,
}

impl HumanRenderOptions {
    pub(crate) fn normalized(self) -> Self {
        Self {
            width: self.width.clamp(MIN_HUMAN_WIDTH, MAX_HUMAN_WIDTH),
            color: self.color,
        }
    }
}

impl Default for HumanRenderOptions {
    fn default() -> Self {
        Self {
            width: DEFAULT_HUMAN_WIDTH,
            color: false,
        }
    }
}

fn safe_human_text(value: &str, fallback: &str) -> String {
    let scrubbed = crate::structured_failures::scrub_absolute_paths(value);
    let mut out = String::with_capacity(scrubbed.len());
    let mut previous_space = false;
    for ch in scrubbed.chars() {
        let normalized = if ch.is_control() { ' ' } else { ch };
        if normalized.is_whitespace() {
            if !previous_space && !out.is_empty() {
                out.push(' ');
            }
            previous_space = true;
        } else {
            out.push(normalized);
            previous_space = false;
        }
    }
    let trimmed = out.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.chars().take(512).collect()
    }
}

fn nested_string<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    match value {
        Value::Object(values) => {
            for key in keys {
                if let Some(text) = values.get(*key).and_then(Value::as_str)
                    && !text.trim().is_empty()
                {
                    return Some(text);
                }
            }
            for key in ["facts", "legacy_context", "cause", "causes", "error"] {
                if let Some(found) = values
                    .get(key)
                    .and_then(|nested| nested_string(nested, keys))
                {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(values) => values.iter().find_map(|nested| nested_string(nested, keys)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    }
}

fn diagnostic_context(diagnostic: &Value) -> Option<&Value> {
    diagnostic
        .get("parameters")
        .and_then(|parameters| parameters.get("context"))
        .or_else(|| diagnostic.get("context"))
}

fn human_operation(diagnostic: &Value) -> String {
    let context = diagnostic_context(diagnostic);
    let contextual = context
        .and_then(|value| value.get("operation"))
        .and_then(Value::as_str)
        .filter(|operation| *operation != "diagnostics/normalize");
    let fallback = diagnostic
        .get("parameters")
        .and_then(|parameters| parameters.get("reported_code"))
        .and_then(Value::as_str)
        .or_else(|| diagnostic.get("code").and_then(Value::as_str))
        .unwrap_or("unknown-operation");
    safe_human_text(contextual.unwrap_or(fallback), "unknown-operation")
}

fn human_subject(diagnostic: &Value) -> String {
    let from_span = diagnostic
        .get("primary_span")
        .and_then(|span| span.get("source"))
        .and_then(Value::as_str);
    let from_context = diagnostic_context(diagnostic).and_then(|context| {
        nested_string(
            context,
            &[
                "subject",
                "source",
                "path",
                "module",
                "package",
                "manifest",
                "obligation",
                "effect_op",
                "blocking_capability",
                "protocol_code",
            ],
        )
    });
    let blocking = diagnostic
        .get("blocking_capability")
        .and_then(Value::as_str);
    safe_human_text(
        from_span
            .or(from_context)
            .or(blocking)
            .unwrap_or("<unknown>"),
        "<unknown>",
    )
}

fn human_cause(diagnostic: &Value) -> String {
    let contextual = diagnostic_context(diagnostic)
        .and_then(|context| nested_string(context, &["reason", "message", "detail"]));
    let producer = diagnostic.get("message").and_then(Value::as_str);
    let catalog = diagnostic
        .get("likely_causes")
        .and_then(Value::as_array)
        .and_then(|causes| causes.first())
        .and_then(Value::as_str);
    safe_human_text(
        contextual
            .or(producer)
            .or(catalog)
            .unwrap_or("No safe cause was reported."),
        "No safe cause was reported.",
    )
}

fn human_action(diagnostic: &Value) -> String {
    let direct = diagnostic.get("next_safe_action").and_then(Value::as_str);
    let catalog = diagnostic
        .get("safe_repair_actions")
        .and_then(Value::as_array)
        .and_then(|actions| actions.first())
        .and_then(|action| action.get("description"))
        .and_then(Value::as_str);
    safe_human_text(
        direct
            .or(catalog)
            .unwrap_or("Preserve the failure context and report the diagnostic code."),
        "Preserve the failure context and report the diagnostic code.",
    )
}

fn split_word(word: &str, width: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in word.chars() {
        current.push(ch);
        if current.chars().count() == width {
            chunks.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn wrap_field(label: &str, text: &str, width: usize) -> Vec<String> {
    let prefix = format!("  {label}: ");
    let continuation = " ".repeat(prefix.chars().count());
    let content_width = width.saturating_sub(prefix.chars().count()).max(1);
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        for chunk in split_word(word, content_width) {
            let needed =
                current.chars().count() + usize::from(!current.is_empty()) + chunk.chars().count();
            if needed > content_width && !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(&chunk);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push("<unknown>".to_string());
    }
    lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                format!("{prefix}{line}")
            } else {
                format!("{continuation}{line}")
            }
        })
        .collect()
}

pub(crate) fn render_human_diagnostic(diagnostic: &Value, options: HumanRenderOptions) -> String {
    let options = options.normalized();
    let code = safe_human_text(
        diagnostic
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or(CATALOG_MISS_CODE),
        CATALOG_MISS_CODE,
    );
    let headline = format!(
        "error[{code}]: {} failed for {}",
        human_operation(diagnostic),
        human_subject(diagnostic)
    );
    let mut lines = wrap_field("error", &headline, options.width);
    if let Some(first) = lines.first_mut() {
        *first = first.replacen("  error: ", "", 1);
    }
    lines.extend(wrap_field("cause", &human_cause(diagnostic), options.width));
    lines.extend(wrap_field("next", &human_action(diagnostic), options.width));
    if options.color {
        for line in &mut lines {
            if line.starts_with("error[") {
                *line = format!("\u{1b}[1;31m{line}\u{1b}[0m");
            } else if let Some(rest) = line.strip_prefix("  cause:") {
                *line = format!("  \u{1b}[1mcause:\u{1b}[0m{rest}");
            } else if let Some(rest) = line.strip_prefix("  next:") {
                *line = format!("  \u{1b}[1mnext:\u{1b}[0m{rest}");
            }
        }
    }
    lines.join("\n")
}

pub(crate) fn render_human_envelope(
    envelope: &Value,
    options: HumanRenderOptions,
) -> Option<String> {
    envelope
        .get("diagnostics")
        .and_then(Value::as_array)
        .and_then(|diagnostics| diagnostics.first())
        .map(|diagnostic| render_human_diagnostic(diagnostic, options))
}

pub(crate) fn render_human_error(
    code: &str,
    message: &str,
    context: Option<Value>,
    exit_code: u8,
    options: HumanRenderOptions,
) -> String {
    let normalized_context = crate::structured_failures::normalize_context(code, context);
    let error = serde_json::json!({
        "code": code,
        "context": normalized_context,
    });
    let blocking_capability = error
        .as_object()
        .map(|values| blocking_capability_from_context(values, code))
        .unwrap_or_default();
    let diagnostic = build_diagnostic(
        code,
        &crate::structured_failures::scrub_absolute_paths(message),
        exit_code,
        error.get("context").cloned(),
        blocking_capability,
    );
    render_human_diagnostic(&diagnostic, options)
}
