use super::*;
use crate::policy::{
    AuthorizedBindPorts, AuthorizedMaxBytes, AuthorizedOptionalBool, AuthorizedOptionalString,
    AuthorizedStringList,
};

fn authorized_string_list(
    state: &AuthorizedStringList,
    key: &str,
    missing: &str,
    empty: &str,
) -> Result<Vec<String>, String> {
    match state {
        AuthorizedStringList::Absent => Err(missing.to_string()),
        AuthorizedStringList::InvalidType => Err(format!("{key} must be an array of strings")),
        AuthorizedStringList::InvalidEntry => Err(format!("{key} entries must be strings")),
        AuthorizedStringList::Empty => Err(empty.to_string()),
        AuthorizedStringList::Valid(values) => Ok(values.clone()),
    }
}

fn net_allowlist_from_policy(pol: Option<&OpPolicy>, op: &str) -> Result<Vec<String>, String> {
    let Some(pol) = pol else {
        return Err(format!(
            "{op} requires per-op url_allow allowlist in caps.toml"
        ));
    };
    if let Some(authorized) = &pol.authorized_network {
        let (key, state) = if authorized.url_allow != AuthorizedStringList::Absent {
            ("url_allow", &authorized.url_allow)
        } else {
            ("remote_allow", &authorized.remote_allow)
        };
        return authorized_string_list(
            state,
            key,
            &format!("{op} requires per-op url_allow allowlist in caps.toml"),
            "url_allow must contain at least one URL prefix",
        );
    }
    let allow_key = if pol.extra.contains_key("url_allow") {
        "url_allow"
    } else {
        "remote_allow"
    };
    let Some(v) = pol.extra.get(allow_key) else {
        return Err(format!(
            "{op} requires per-op url_allow allowlist in caps.toml"
        ));
    };
    let Some(arr) = v.as_array() else {
        return Err(format!("{allow_key} must be an array of strings"));
    };
    let mut out = Vec::with_capacity(arr.len());
    for x in arr {
        let Some(raw) = x.as_str() else {
            return Err(format!("{allow_key} entries must be strings"));
        };
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
    }
    if out.is_empty() {
        return Err("url_allow must contain at least one URL prefix".to_string());
    }
    Ok(out)
}

fn net_allow_http_from_policy(pol: Option<&OpPolicy>) -> Result<bool, String> {
    let Some(pol) = pol else {
        return Ok(false);
    };
    if let Some(authorized) = &pol.authorized_network {
        return match &authorized.allow_http {
            AuthorizedOptionalBool::Absent => Ok(false),
            AuthorizedOptionalBool::InvalidType => Err("allow_http must be a boolean".to_string()),
            AuthorizedOptionalBool::Valid(value) => Ok(*value),
        };
    }
    let Some(v) = pol.extra.get("allow_http") else {
        return Ok(false);
    };
    let Some(allow_http) = v.as_bool() else {
        return Err("allow_http must be a boolean".to_string());
    };
    Ok(allow_http)
}

fn net_wasi_network_profile_from_policy(pol: Option<&OpPolicy>) -> Result<Option<String>, String> {
    let Some(pol) = pol else {
        return Ok(None);
    };
    if let Some(authorized) = &pol.authorized_network {
        return match &authorized.wasi_network_profile {
            AuthorizedOptionalString::Absent => Ok(None),
            AuthorizedOptionalString::InvalidType => {
                Err("wasi_network_profile must be a string".to_string())
            }
            AuthorizedOptionalString::Empty => {
                Err("wasi_network_profile must not be empty".to_string())
            }
            AuthorizedOptionalString::Valid(value) => Ok(Some(value.clone())),
        };
    }
    let Some(v) = pol.extra.get("wasi_network_profile") else {
        return Ok(None);
    };
    let Some(raw) = v.as_str() else {
        return Err("wasi_network_profile must be a string".to_string());
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("wasi_network_profile must not be empty".to_string());
    }
    Ok(Some(trimmed.to_string()))
}

fn net_bind_hosts_from_policy(pol: Option<&OpPolicy>, op: &str) -> Result<Vec<String>, String> {
    if let Some(authorized) = pol.and_then(|policy| policy.authorized_network.as_ref()) {
        return authorized_string_list(
            &authorized.bind_hosts,
            "allow_bind_hosts",
            &format!("{op} requires per-op allow_bind_hosts allowlist in caps.toml"),
            "allow_bind_hosts must contain at least one entry",
        );
    }
    parse_nonempty_string_array(
        pol,
        "allow_bind_hosts",
        &format!("{op} requires per-op allow_bind_hosts allowlist in caps.toml"),
    )
}

#[derive(Debug, Clone)]
struct BindPortAllowlist {
    any: bool,
    ports: Vec<u16>,
}

fn parse_nonempty_u16_array(
    pol: Option<&OpPolicy>,
    key: &str,
    missing_msg: &str,
) -> Result<BindPortAllowlist, String> {
    let Some(pol) = pol else {
        return Err(missing_msg.to_string());
    };
    let Some(v) = pol.extra.get(key) else {
        return Err(missing_msg.to_string());
    };
    let Some(arr) = v.as_array() else {
        return Err(format!("{key} must be an array of integers"));
    };
    let mut out = Vec::with_capacity(arr.len());
    let mut any = false;
    for x in arr {
        if let Some(raw) = x.as_integer() {
            if !(1..=65535).contains(&raw) {
                return Err(format!("{key} entries must be between 1 and 65535"));
            }
            out.push(raw as u16);
            continue;
        }
        if let Some(raw) = x.as_str()
            && raw.trim() == "*"
        {
            any = true;
            continue;
        }
        return Err(format!("{key} entries must be integers or \"*\""));
    }
    if out.is_empty() && !any {
        return Err(format!("{key} must contain at least one entry"));
    }
    Ok(BindPortAllowlist { any, ports: out })
}

fn net_bind_ports_from_policy(
    pol: Option<&OpPolicy>,
    op: &str,
) -> Result<BindPortAllowlist, String> {
    if let Some(authorized) = pol.and_then(|policy| policy.authorized_network.as_ref()) {
        return match &authorized.bind_ports {
            AuthorizedBindPorts::Absent => Err(format!(
                "{op} requires per-op allow_bind_ports allowlist in caps.toml"
            )),
            AuthorizedBindPorts::InvalidType => {
                Err("allow_bind_ports must be an array of integers".to_string())
            }
            AuthorizedBindPorts::InvalidEntry => {
                Err("allow_bind_ports entries must be integers or \"*\"".to_string())
            }
            AuthorizedBindPorts::OutOfRange => {
                Err("allow_bind_ports entries must be between 1 and 65535".to_string())
            }
            AuthorizedBindPorts::Empty => {
                Err("allow_bind_ports must contain at least one entry".to_string())
            }
            AuthorizedBindPorts::Valid { any, ports } => Ok(BindPortAllowlist {
                any: *any,
                ports: ports.clone(),
            }),
        };
    }
    parse_nonempty_u16_array(
        pol,
        "allow_bind_ports",
        &format!("{op} requires per-op allow_bind_ports allowlist in caps.toml"),
    )
}

pub(super) fn net_max_request_bytes_from_policy(
    pol: Option<&OpPolicy>,
    op: &str,
) -> Result<usize, String> {
    let Some(pol) = pol else {
        return Err(format!(
            "{op} requires per-op max_request_bytes bound in caps.toml"
        ));
    };
    if let Some(authorized) = &pol.authorized_network {
        return match &authorized.max_request_bytes {
            AuthorizedMaxBytes::Absent => Err(format!(
                "{op} requires per-op max_request_bytes bound in caps.toml"
            )),
            AuthorizedMaxBytes::InvalidType => {
                Err("max_request_bytes must be an integer".to_string())
            }
            AuthorizedMaxBytes::NonPositive => {
                Err("max_request_bytes must be greater than zero".to_string())
            }
            AuthorizedMaxBytes::PlatformOverflow => {
                Err("max_request_bytes exceeds platform usize range".to_string())
            }
            AuthorizedMaxBytes::Valid(value) => Ok(*value),
        };
    }
    let Some(v) = pol.extra.get("max_request_bytes") else {
        return Err(format!(
            "{op} requires per-op max_request_bytes bound in caps.toml"
        ));
    };
    let Some(raw) = v.as_integer() else {
        return Err("max_request_bytes must be an integer".to_string());
    };
    if raw <= 0 {
        return Err("max_request_bytes must be greater than zero".to_string());
    }
    let Ok(bound) = usize::try_from(raw) else {
        return Err("max_request_bytes exceeds platform usize range".to_string());
    };
    Ok(bound)
}

fn validate_net_wasi_profile(profile: Option<&str>, scheme: &str) -> Result<(), String> {
    if !cfg!(target_os = "wasi") {
        return Ok(());
    }
    let profile = profile.unwrap_or("none");
    match profile {
        "none" => Err("WASI network access is disabled; set wasi_network_profile to `local` or `preview2` in caps.toml op policy".to_string()),
        "local" => {
            if matches!(scheme, "file" | "inproc")
                || (matches!(scheme, "http" | "https") && gc_registry::wasi_http_bridge_configured())
            {
                Ok(())
            } else {
                Err(format!(
                    "wasi_network_profile=local only allows file:// or inproc:// URLs (got scheme `{scheme}`)"
                ))
            }
        }
        "preview2" => Ok(()),
        other => Err(format!(
            "invalid wasi_network_profile `{other}`; expected `none`, `local`, or `preview2`"
        )),
    }
}

fn parse_bind_host_port(target: &str, op: &str, field: &str) -> Result<(String, u16), String> {
    let Some((_scheme, rest)) = target.split_once("://") else {
        return Err(format!(
            "{op} payload field `{field}` must include scheme:// (got `{target}`)"
        ));
    };
    let authority = rest.split('/').next().unwrap_or_default().trim();
    if authority.is_empty() {
        return Err(format!(
            "{op} payload field `{field}` must include bind host:port"
        ));
    }
    if let Some(stripped) = authority.strip_prefix('[') {
        let Some((host, port_part)) = stripped.split_once(']') else {
            return Err(format!(
                "{op} payload field `{field}` has invalid IPv6 bind authority `{authority}`"
            ));
        };
        let Some(port_raw) = port_part.strip_prefix(':') else {
            return Err(format!(
                "{op} payload field `{field}` must include bind port in authority `{authority}`"
            ));
        };
        let port = port_raw.parse::<u16>().map_err(|_| {
            format!("{op} payload field `{field}` has invalid bind port `{port_raw}`")
        })?;
        if host.trim().is_empty() {
            return Err(format!(
                "{op} payload field `{field}` has empty bind host in authority `{authority}`"
            ));
        }
        return Ok((host.trim().to_lowercase(), port));
    }
    let Some((host_raw, port_raw)) = authority.rsplit_once(':') else {
        return Err(format!(
            "{op} payload field `{field}` must include bind host:port in authority `{authority}`"
        ));
    };
    let host = host_raw.trim();
    if host.is_empty() {
        return Err(format!(
            "{op} payload field `{field}` has empty bind host in authority `{authority}`"
        ));
    }
    let port = port_raw
        .parse::<u16>()
        .map_err(|_| format!("{op} payload field `{field}` has invalid bind port `{port_raw}`"))?;
    Ok((host.to_lowercase(), port))
}

pub(super) fn validate_net_bind_policy(
    pol: Option<&OpPolicy>,
    target: &str,
    op: &str,
    field: &str,
) -> Result<(), String> {
    let (bind_host, bind_port) = parse_bind_host_port(target, op, field)?;
    let allow_hosts = net_bind_hosts_from_policy(pol, op)?;
    let allow_ports = net_bind_ports_from_policy(pol, op)?;
    let host_ok = allow_hosts
        .iter()
        .any(|candidate| allowlist_rule_exact_or_glob_matches_ci(candidate, &bind_host));
    if !host_ok {
        return Err(format!(
            "bind host `{bind_host}` is not in allow_bind_hosts policy"
        ));
    }
    if !allow_ports.any && !allow_ports.ports.contains(&bind_port) {
        return Err(format!(
            "bind port `{bind_port}` is not in allow_bind_ports policy"
        ));
    }
    Ok(())
}

fn url_matches_allowlist(url: &str, allow: &str, scheme: &str) -> bool {
    let rule = allow.trim();
    if rule == "*" {
        return true;
    }
    if rule.ends_with("://") {
        return scheme == rule.trim_end_matches("://");
    }
    allowlist_rule_prefix_or_glob_matches(rule, url)
}

pub(super) fn validate_net_target_policy(
    pol: Option<&OpPolicy>,
    target: &str,
    op: &str,
    field: &str,
) -> Result<(), String> {
    let scheme = parse_url_scheme(target, op, field)?;
    let allow_http = net_allow_http_from_policy(pol)?;
    if scheme == "http" && !allow_http {
        return Err("http URLs are disabled by policy (set allow_http=true)".to_string());
    }
    let wasi_profile = net_wasi_network_profile_from_policy(pol)?;
    validate_net_wasi_profile(wasi_profile.as_deref(), scheme)?;
    let allowlist = net_allowlist_from_policy(pol, op)?;
    if allowlist
        .iter()
        .any(|rule| url_matches_allowlist(target, rule, scheme))
    {
        return Ok(());
    }
    Err("target is not in policy url_allow allowlist".to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use toml::Value as TomlValue;

    use super::*;
    use crate::policy::AuthorizedNetworkPolicy;

    fn network_policy() -> AuthorizedNetworkPolicy {
        AuthorizedNetworkPolicy {
            url_allow: AuthorizedStringList::Absent,
            remote_allow: AuthorizedStringList::Absent,
            allow_http: AuthorizedOptionalBool::Absent,
            wasi_network_profile: AuthorizedOptionalString::Absent,
            bind_hosts: AuthorizedStringList::Absent,
            bind_ports: AuthorizedBindPorts::Absent,
            max_request_bytes: AuthorizedMaxBytes::Absent,
        }
    }

    fn policy_with(
        extra: BTreeMap<String, TomlValue>,
        network: AuthorizedNetworkPolicy,
    ) -> OpPolicy {
        OpPolicy {
            base_dir: None,
            create_dirs: false,
            timeout_ms: None,
            log_inline_max_bytes: None,
            extra,
            authorized_cap: None,
            authorized_max_bytes: None,
            authorized_process_programs: None,
            authorized_database: None,
            authorized_network: Some(network),
            authorized_crypto: None,
            authorized_plugin: None,
            authorized_ffi: None,
        }
    }

    #[test]
    fn network_dispatch_consumes_authorized_policy_before_raw_policy() {
        let mut network = network_policy();
        network.url_allow = AuthorizedStringList::Valid(vec!["https://safe.example/".to_string()]);
        network.allow_http = AuthorizedOptionalBool::Valid(false);
        network.bind_hosts = AuthorizedStringList::Valid(vec!["127.0.0.1".to_string()]);
        network.bind_ports = AuthorizedBindPorts::Valid {
            any: false,
            ports: vec![8080],
        };
        network.max_request_bytes = AuthorizedMaxBytes::Valid(4096);
        let raw = BTreeMap::from([
            (
                "url_allow".to_string(),
                TomlValue::String("invalid raw fallback".to_string()),
            ),
            (
                "allow_http".to_string(),
                TomlValue::String("invalid raw fallback".to_string()),
            ),
            (
                "allow_bind_hosts".to_string(),
                TomlValue::String("invalid raw fallback".to_string()),
            ),
            (
                "allow_bind_ports".to_string(),
                TomlValue::String("invalid raw fallback".to_string()),
            ),
            (
                "max_request_bytes".to_string(),
                TomlValue::String("invalid raw fallback".to_string()),
            ),
        ]);
        let policy = policy_with(raw, network);
        assert!(
            validate_net_target_policy(
                Some(&policy),
                "https://safe.example/api",
                "io/net::http-request",
                ":url",
            )
            .is_ok()
        );
        assert!(
            validate_net_bind_policy(
                Some(&policy),
                "tcp://127.0.0.1:8080",
                "io/net::tcp-listen",
                ":local",
            )
            .is_ok()
        );
        assert_eq!(
            net_max_request_bytes_from_policy(Some(&policy), "io/net::tcp-accept"),
            Ok(4096)
        );
    }

    #[test]
    fn network_dispatch_preserves_authorized_policy_errors() {
        let mut network = network_policy();
        network.url_allow = AuthorizedStringList::InvalidEntry;
        network.allow_http = AuthorizedOptionalBool::InvalidType;
        network.bind_ports = AuthorizedBindPorts::OutOfRange;
        network.max_request_bytes = AuthorizedMaxBytes::NonPositive;
        let policy = policy_with(BTreeMap::new(), network);
        assert_eq!(
            net_allowlist_from_policy(Some(&policy), "io/net::http-request").unwrap_err(),
            "url_allow entries must be strings"
        );
        assert_eq!(
            net_allow_http_from_policy(Some(&policy)).unwrap_err(),
            "allow_http must be a boolean"
        );
        assert_eq!(
            net_bind_ports_from_policy(Some(&policy), "io/net::tcp-listen").unwrap_err(),
            "allow_bind_ports entries must be between 1 and 65535"
        );
        assert_eq!(
            net_max_request_bytes_from_policy(Some(&policy), "io/net::tcp-accept").unwrap_err(),
            "max_request_bytes must be greater than zero"
        );
    }
}
