use super::*;
use gc_prelude::SelfhostBootstrapMode;

fn frontend() -> CoreformFrontend {
    let artifact = std::env::var_os("GENESIS_TEST_SELFHOST_ARTIFACT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join("selfhost/toolchain.gc")
        });
    CoreformFrontend::Selfhost(SelfhostFrontendConfig {
        bootstrap_mode: SelfhostBootstrapMode::ArtifactOnly,
        artifact: Some(artifact),
    })
}

#[test]
fn package_manifest_authority_normalizes_complete_manifest() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("package.toml");
    std::fs::write(
        &path,
        r#"
schema = 1
name = "demo"
version = "1.2.3"
obligations = ["core/obligation::unit-tests"]
tests = ["demo/tests::unit"]
property_tests = ["demo/tests::property"]
caps_policy = "policy/caps.toml"

[[modules]]
path = "src/main.gc"
hash = "abc"

[[dependencies]]
name = "dep"
path = "deps/dep"

[limits]
step_limit = 123
allow_unlimited = true
max_alloc_units = 456

[budgets]
max_steps_per_test = 789

[property]
cases_per_test = 32

[gfx]
golden_tests = ["demo/tests::gfx"]
api_exports = ["demo/gfx::draw"]
max_frame_time_ms = 16
"#,
    )
    .unwrap();

    let (manifest, directory) =
        load_package_manifest_with_frontend(&path, &frontend()).expect("authorized manifest");
    assert_eq!(directory, temp.path());
    assert_eq!(manifest.schema, 1);
    assert_eq!(manifest.name, "demo");
    assert_eq!(manifest.modules[0].path, "src/main.gc");
    assert_eq!(manifest.dependencies[0].path, "deps/dep");
    assert_eq!(manifest.caps_policy.as_deref(), Some("policy/caps.toml"));
    assert_eq!(manifest.limits.step_limit, Some(123));
    assert!(manifest.limits.allow_unlimited);
    assert_eq!(manifest.budgets.max_steps_per_test, Some(789));
    assert_eq!(manifest.property.cases_per_test, Some(32));
    assert_eq!(manifest.gfx.max_frame_time_ms, Some(16));
}

#[test]
fn package_manifest_authority_preserves_legacy_defaults() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("package.toml");
    std::fs::write(
        &path,
        "name = \"legacy\"\nversion = \"0.1.0\"\nmodules = []\nobligations = []\n",
    )
    .unwrap();
    let (manifest, _) =
        load_package_manifest_with_frontend(&path, &frontend()).expect("legacy manifest");
    assert_eq!(manifest.schema, 1);
    assert!(manifest.dependencies.is_empty());
    assert!(manifest.tests.is_empty());
    assert!(!manifest.limits.allow_unlimited);
    assert!(manifest.gfx.api_exports.is_empty());
}

#[test]
fn package_manifest_authority_preserves_schema_repair_contract() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("package.toml");
    std::fs::write(
        &path,
        "schema = 2\nname = \"future\"\nversion = \"0.1.0\"\nmodules = []\nobligations = []\n",
    )
    .unwrap();
    let error = load_package_manifest_with_frontend(&path, &frontend())
        .expect_err("unsupported schema must fail")
        .to_string();
    assert!(
        error.contains("unsupported package manifest schema"),
        "{error}"
    );
}

#[test]
fn package_manifest_authority_rejects_nonportable_paths() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("package.toml");
    for invalid in ["../escape.gc", "src//main.gc", "C:/main.gc", "src\\main.gc"] {
        std::fs::write(
            &path,
            format!(
                "name = \"bad\"\nversion = \"0.1.0\"\nobligations = []\n[[modules]]\npath = {invalid:?}\n"
            ),
        )
        .unwrap();
        let error = load_package_manifest_with_frontend(&path, &frontend())
            .expect_err("nonportable path must fail")
            .to_string();
        assert!(
            error.contains("portable package-relative"),
            "{invalid}: {error}"
        );
    }
}

#[test]
fn package_manifest_authority_rejects_invalid_types_without_source_access() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("package.toml");
    std::fs::write(
        &path,
        "name = \"bad\"\nversion = \"0.1.0\"\nmodules = [\"missing.gc\"]\nobligations = []\n",
    )
    .unwrap();
    let error = load_package_manifest_with_frontend(&path, &frontend())
        .expect_err("invalid module shape must fail")
        .to_string();
    assert!(error.contains("module entry must be a table"), "{error}");
    assert!(!temp.path().join("missing.gc").exists());
}
