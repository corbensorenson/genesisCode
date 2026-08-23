use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use gc_coreform::{Term, TermOrdKey};

use crate::manifest::{
    Budgets, DepEntry, GfxConfig, Limits, ManifestError, ModuleEntry, PackageManifest,
    PropertyConfig,
};

pub const PACKAGE_MANIFEST_AUTHORITY_BINDING: &str = "core/pkg::package-manifest-authority";
pub const PACKAGE_MANIFEST_AUTHORITY_REQUEST_KIND: &str =
    "genesis/pkg-package-manifest-authority-request-v0.1";
pub const PACKAGE_MANIFEST_AUTHORITY_RESULT_KIND: &str =
    "genesis/pkg-package-manifest-authority-result-v0.1";

const SOURCE_LIMIT: u64 = 16 * 1024 * 1024;
const COLLECTION_LIMIT: usize = 4096;
const STRING_LIMIT: usize = 16 * 1024 * 1024;

pub struct PackageManifestTransport {
    pub document: Term,
    pub source_hash: String,
    pub package_dir: PathBuf,
}

pub fn read_package_manifest_transport(
    path: &Path,
) -> Result<PackageManifestTransport, ManifestError> {
    let mut file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(SOURCE_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > SOURCE_LIMIT {
        return Err(invalid(
            path,
            format!("manifest exceeds {SOURCE_LIMIT}-byte transport limit"),
        ));
    }
    let source = std::str::from_utf8(&bytes).map_err(|_| ManifestError::Parse {
        path: path.display().to_string(),
        msg: "manifest is not valid UTF-8".to_string(),
    })?;
    let document = toml::from_str::<toml::Value>(source).map_err(|error| ManifestError::Parse {
        path: path.display().to_string(),
        msg: error.to_string(),
    })?;
    let package_dir = path
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| invalid(path, "package.toml has no parent dir"))?;
    Ok(PackageManifestTransport {
        document: toml_to_term(document),
        source_hash: blake3::hash(&bytes).to_hex().to_string(),
        package_dir,
    })
}

pub fn package_manifest_authority_request(document: Term, source_hash: &str) -> Term {
    map([
        (":document", document),
        (
            ":kind",
            Term::Str(PACKAGE_MANIFEST_AUTHORITY_REQUEST_KIND.to_string()),
        ),
        (":source-h", Term::Str(source_hash.to_string())),
        (":v", Term::Int(1.into())),
    ])
}

pub fn decode_authorized_package_manifest(
    path: &Path,
    value: Term,
    request_hash: &str,
    source_hash: &str,
) -> Result<PackageManifest, ManifestError> {
    let envelope = required_map(path, &value, "authority envelope")?;
    exact_fields(
        path,
        envelope,
        &[
            ":code",
            ":kind",
            ":message",
            ":ok",
            ":request-h",
            ":v",
            ":value",
        ],
        "authority envelope",
    )?;
    require_string(
        path,
        envelope,
        ":kind",
        PACKAGE_MANIFEST_AUTHORITY_RESULT_KIND,
    )?;
    require_string(path, envelope, ":request-h", request_hash)?;
    require_u64(path, envelope, ":v", 1)?;
    match field(path, envelope, ":ok")? {
        Term::Bool(false) => {
            require_nil(path, envelope, ":value")?;
            require_string(path, envelope, ":code", "core/pkg/bad-package-manifest")?;
            return Err(invalid(
                path,
                format!(
                    "core/pkg/bad-package-manifest: {}",
                    required_string(path, envelope, ":message")?
                ),
            ));
        }
        Term::Bool(true) => {
            require_nil(path, envelope, ":code")?;
            require_nil(path, envelope, ":message")?;
        }
        _ => return Err(invalid(path, "authority envelope :ok must be bool")),
    }

    let result = required_map(path, field(path, envelope, ":value")?, "authority result")?;
    exact_fields(
        path,
        result,
        &[
            ":budgets",
            ":caps-policy",
            ":dependencies",
            ":gfx",
            ":limits",
            ":modules",
            ":name",
            ":obligations",
            ":property",
            ":property-tests",
            ":schema",
            ":source-h",
            ":tests",
            ":version",
        ],
        "authority result",
    )?;
    require_string(path, result, ":source-h", source_hash)?;
    Ok(PackageManifest {
        schema: required_u64(path, result, ":schema")?,
        name: bounded_string(path, result, ":name")?,
        version: bounded_string(path, result, ":version")?,
        modules: decode_modules(path, field(path, result, ":modules")?)?,
        dependencies: decode_dependencies(path, field(path, result, ":dependencies")?)?,
        obligations: decode_strings(path, field(path, result, ":obligations")?, "obligations")?,
        tests: decode_strings(path, field(path, result, ":tests")?, "tests")?,
        property_tests: decode_strings(
            path,
            field(path, result, ":property-tests")?,
            "property tests",
        )?,
        caps_policy: optional_string(path, result, ":caps-policy")?,
        limits: decode_limits(path, field(path, result, ":limits")?)?,
        budgets: decode_budgets(path, field(path, result, ":budgets")?)?,
        property: decode_property(path, field(path, result, ":property")?)?,
        gfx: decode_gfx(path, field(path, result, ":gfx")?)?,
    })
}

fn decode_modules(path: &Path, term: &Term) -> Result<Vec<ModuleEntry>, ManifestError> {
    bounded_vector(path, term, "modules")?
        .iter()
        .map(|value| {
            let fields = required_map(path, value, "module entry")?;
            exact_fields(path, fields, &[":hash", ":path"], "module entry")?;
            Ok(ModuleEntry {
                path: bounded_string(path, fields, ":path")?,
                hash: optional_string(path, fields, ":hash")?,
            })
        })
        .collect()
}

fn decode_dependencies(path: &Path, term: &Term) -> Result<Vec<DepEntry>, ManifestError> {
    bounded_vector(path, term, "dependencies")?
        .iter()
        .map(|value| {
            let fields = required_map(path, value, "dependency entry")?;
            exact_fields(
                path,
                fields,
                &[":hash", ":name", ":path"],
                "dependency entry",
            )?;
            Ok(DepEntry {
                name: bounded_string(path, fields, ":name")?,
                path: bounded_string(path, fields, ":path")?,
                hash: optional_string(path, fields, ":hash")?,
            })
        })
        .collect()
}

fn decode_limits(path: &Path, term: &Term) -> Result<Limits, ManifestError> {
    let fields = required_map(path, term, "limits")?;
    exact_fields(
        path,
        fields,
        &[
            ":allow-unlimited",
            ":max-alloc-units",
            ":max-bytes-len",
            ":max-live-units",
            ":max-map-len",
            ":max-pair-cells",
            ":max-string-len",
            ":max-vec-len",
            ":step-limit",
        ],
        "limits",
    )?;
    Ok(Limits {
        step_limit: optional_u64(path, fields, ":step-limit")?,
        allow_unlimited: required_bool(path, fields, ":allow-unlimited")?,
        max_alloc_units: optional_u64(path, fields, ":max-alloc-units")?,
        max_live_units: optional_u64(path, fields, ":max-live-units")?,
        max_pair_cells: optional_u64(path, fields, ":max-pair-cells")?,
        max_vec_len: optional_u64(path, fields, ":max-vec-len")?,
        max_map_len: optional_u64(path, fields, ":max-map-len")?,
        max_bytes_len: optional_u64(path, fields, ":max-bytes-len")?,
        max_string_len: optional_u64(path, fields, ":max-string-len")?,
    })
}

fn decode_budgets(path: &Path, term: &Term) -> Result<Budgets, ManifestError> {
    let fields = required_map(path, term, "budgets")?;
    exact_fields(
        path,
        fields,
        &[
            ":max-effect-entries-per-test",
            ":max-effect-log-bytes-per-test",
            ":max-steps-per-test",
        ],
        "budgets",
    )?;
    Ok(Budgets {
        max_steps_per_test: optional_u64(path, fields, ":max-steps-per-test")?,
        max_effect_entries_per_test: optional_u64(path, fields, ":max-effect-entries-per-test")?,
        max_effect_log_bytes_per_test: optional_u64(
            path,
            fields,
            ":max-effect-log-bytes-per-test",
        )?,
    })
}

fn decode_property(path: &Path, term: &Term) -> Result<PropertyConfig, ManifestError> {
    let fields = required_map(path, term, "property")?;
    exact_fields(path, fields, &[":cases-per-test"], "property")?;
    Ok(PropertyConfig {
        cases_per_test: optional_u64(path, fields, ":cases-per-test")?,
    })
}

fn decode_gfx(path: &Path, term: &Term) -> Result<GfxConfig, ManifestError> {
    let fields = required_map(path, term, "gfx")?;
    exact_fields(
        path,
        fields,
        &[
            ":api-exports",
            ":api-surface-hash",
            ":frame-budget-tests",
            ":golden-tests",
            ":max-compute-commands-per-frame",
            ":max-compute-passes-per-frame",
            ":max-draw-commands-per-frame",
            ":max-frame-graph-bytes",
            ":max-frame-time-ms",
            ":max-render-passes-per-frame",
        ],
        "gfx",
    )?;
    Ok(GfxConfig {
        golden_tests: decode_strings(path, field(path, fields, ":golden-tests")?, "golden tests")?,
        frame_budget_tests: decode_strings(
            path,
            field(path, fields, ":frame-budget-tests")?,
            "frame budget tests",
        )?,
        api_exports: decode_strings(path, field(path, fields, ":api-exports")?, "API exports")?,
        api_surface_hash: optional_string(path, fields, ":api-surface-hash")?,
        max_render_passes_per_frame: optional_u64(path, fields, ":max-render-passes-per-frame")?,
        max_compute_passes_per_frame: optional_u64(path, fields, ":max-compute-passes-per-frame")?,
        max_draw_commands_per_frame: optional_u64(path, fields, ":max-draw-commands-per-frame")?,
        max_compute_commands_per_frame: optional_u64(
            path,
            fields,
            ":max-compute-commands-per-frame",
        )?,
        max_frame_graph_bytes: optional_u64(path, fields, ":max-frame-graph-bytes")?,
        max_frame_time_ms: optional_u64(path, fields, ":max-frame-time-ms")?,
    })
}

fn decode_strings(path: &Path, term: &Term, label: &str) -> Result<Vec<String>, ManifestError> {
    bounded_vector(path, term, label)?
        .iter()
        .map(|value| match value {
            Term::Str(value) if value.len() <= STRING_LIMIT => Ok(value.clone()),
            _ => Err(invalid(
                path,
                format!("{label} entries must be bounded strings"),
            )),
        })
        .collect()
}

fn bounded_vector<'a>(
    path: &Path,
    term: &'a Term,
    label: &str,
) -> Result<&'a [Term], ManifestError> {
    match term {
        Term::Vector(values) if values.len() <= COLLECTION_LIMIT => Ok(values),
        Term::Vector(_) => Err(invalid(
            path,
            format!("{label} exceeds {COLLECTION_LIMIT}-entry result limit"),
        )),
        _ => Err(invalid(path, format!("{label} must be vector"))),
    }
}

fn bounded_string(
    path: &Path,
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<String, ManifestError> {
    let value = required_string(path, fields, name)?;
    if value.len() > STRING_LIMIT {
        return Err(invalid(
            path,
            format!("{name} exceeds {STRING_LIMIT}-byte limit"),
        ));
    }
    Ok(value.to_string())
}

fn optional_string(
    path: &Path,
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<Option<String>, ManifestError> {
    match field(path, fields, name)? {
        Term::Nil => Ok(None),
        Term::Str(value) if value.len() <= STRING_LIMIT => Ok(Some(value.clone())),
        _ => Err(invalid(
            path,
            format!("{name} must be bounded string or nil"),
        )),
    }
}

fn optional_u64(
    path: &Path,
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<Option<u64>, ManifestError> {
    match field(path, fields, name)? {
        Term::Nil => Ok(None),
        Term::Int(value) => u64::try_from(value.clone()).map(Some).map_err(|_| {
            invalid(
                path,
                format!("{name} must be unsigned 64-bit integer or nil"),
            )
        }),
        _ => Err(invalid(
            path,
            format!("{name} must be unsigned 64-bit integer or nil"),
        )),
    }
}

fn required_u64(
    path: &Path,
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<u64, ManifestError> {
    match field(path, fields, name)? {
        Term::Int(value) => u64::try_from(value.clone())
            .map_err(|_| invalid(path, format!("{name} must be unsigned 64-bit integer"))),
        _ => Err(invalid(
            path,
            format!("{name} must be unsigned 64-bit integer"),
        )),
    }
}

fn required_bool(
    path: &Path,
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<bool, ManifestError> {
    match field(path, fields, name)? {
        Term::Bool(value) => Ok(*value),
        _ => Err(invalid(path, format!("{name} must be bool"))),
    }
}

fn required_map<'a>(
    path: &Path,
    term: &'a Term,
    label: &str,
) -> Result<&'a BTreeMap<TermOrdKey, Term>, ManifestError> {
    match term {
        Term::Map(fields) => Ok(fields),
        _ => Err(invalid(path, format!("{label} must be map"))),
    }
}

fn exact_fields(
    path: &Path,
    fields: &BTreeMap<TermOrdKey, Term>,
    names: &[&str],
    label: &str,
) -> Result<(), ManifestError> {
    let expected = names
        .iter()
        .map(|name| TermOrdKey(Term::symbol(*name)))
        .collect::<BTreeSet<_>>();
    if fields.keys().cloned().collect::<BTreeSet<_>>() == expected {
        Ok(())
    } else {
        Err(invalid(path, format!("{label} field set mismatch")))
    }
}

fn field<'a>(
    path: &Path,
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<&'a Term, ManifestError> {
    fields
        .get(&TermOrdKey(Term::symbol(name)))
        .ok_or_else(|| invalid(path, format!("authority result missing {name}")))
}

fn required_string<'a>(
    path: &Path,
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<&'a str, ManifestError> {
    match field(path, fields, name)? {
        Term::Str(value) => Ok(value),
        _ => Err(invalid(path, format!("{name} must be string"))),
    }
}

fn require_string(
    path: &Path,
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: &str,
) -> Result<(), ManifestError> {
    if required_string(path, fields, name)? == expected {
        Ok(())
    } else {
        Err(invalid(path, format!("{name} mismatch")))
    }
}

fn require_u64(
    path: &Path,
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: u64,
) -> Result<(), ManifestError> {
    if required_u64(path, fields, name)? == expected {
        Ok(())
    } else {
        Err(invalid(path, format!("{name} mismatch")))
    }
}

fn require_nil(
    path: &Path,
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<(), ManifestError> {
    if matches!(field(path, fields, name)?, Term::Nil) {
        Ok(())
    } else {
        Err(invalid(path, format!("{name} must be nil")))
    }
}

fn invalid(path: &Path, message: impl Into<String>) -> ManifestError {
    ManifestError::Invalid {
        path: path.display().to_string(),
        msg: message.into(),
    }
}

fn map(entries: impl IntoIterator<Item = (&'static str, Term)>) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(name, value)| (TermOrdKey(Term::symbol(name)), value))
            .collect(),
    )
}

fn toml_to_term(value: toml::Value) -> Term {
    match value {
        toml::Value::String(value) => Term::Str(value),
        toml::Value::Integer(value) => Term::Int(value.into()),
        toml::Value::Boolean(value) => Term::Bool(value),
        toml::Value::Float(value) => map([(":toml-float", Term::Str(format!("{value:e}")))]),
        toml::Value::Datetime(value) => map([(":toml-datetime", Term::Str(value.to_string()))]),
        toml::Value::Array(values) => Term::Vector(values.into_iter().map(toml_to_term).collect()),
        toml::Value::Table(values) => Term::Map(
            values
                .into_iter()
                .map(|(key, value)| (TermOrdKey(Term::Str(key)), toml_to_term(value)))
                .collect(),
        ),
    }
}
