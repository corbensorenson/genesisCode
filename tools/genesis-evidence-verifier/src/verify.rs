use crate::Args;
use crate::json::{canonical_bytes, parse_unique};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

const HARD_INPUT_LIMIT: u64 = 16 * 1024 * 1024;
const STATEMENT_TYPE: &str = "https://in-toto.io/Statement/v1";
const GENESIS_PREDICATE: &str = "https://genesiscode.dev/attestations/evidence/v0.1";
const SLSA_PREDICATE: &str = "https://slsa.dev/provenance/v1";
const PAYLOAD_TYPE: &str = "application/vnd.in-toto+json";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Policy {
    kind: String,
    version: String,
    compatibility_profile: String,
    accepted_bundle_profiles: Vec<String>,
    required_predicate_types: Vec<String>,
    signature_policy: SignaturePolicy,
    source_policy: SourcePolicy,
    network_policy: NetworkPolicy,
    negative_control_policy: NegativeControlPolicy,
    artifact_tree_policy: ArtifactTreePolicy,
    compatibility: Compatibility,
    limits: Limits,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SignaturePolicy {
    thresholds_by_profile: BTreeMap<String, usize>,
    predicate_roles: BTreeMap<String, String>,
    trusted_keys: Vec<TrustKey>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct TrustKey {
    keyid: String,
    algorithm: String,
    public_key: String,
    roles: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SourcePolicy {
    require_clean: bool,
    allowed_dirty_policies: Vec<String>,
    expected_repository_uri: String,
    expected_revision: String,
    expected_tree_digest: DigestValue,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct NetworkPolicy {
    required_mode: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct NegativeControlPolicy {
    require_all_passed: bool,
    minimum_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ArtifactTreePolicy {
    required: bool,
    algorithm: String,
    manifest_digest: DigestValue,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Compatibility {
    allowed_build_types: Vec<String>,
    allowed_builder_ids: Vec<String>,
    allowed_environment_profiles: Vec<String>,
    allowed_verifiers: Vec<AllowedVerifier>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct AllowedVerifier {
    name: String,
    version: String,
    artifact_uri: String,
    artifact_digest: DigestValue,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Limits {
    max_input_bytes: u64,
    max_attestations: usize,
    max_signatures_per_attestation: usize,
    max_subjects: usize,
    max_artifacts: usize,
    max_artifact_bytes: u64,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DigestValue {
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ArtifactTree {
    kind: String,
    version: String,
    algorithm: String,
    root_digest: DigestValue,
    entries: Vec<TreeEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct TreeEntry {
    path: String,
    digest: DigestValue,
    size_bytes: u64,
    #[serde(rename = "type")]
    entry_type: String,
}

#[derive(Debug)]
struct TrustedKey {
    keyid: String,
    key: VerifyingKey,
    roles: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Subject {
    name: String,
    sha256: String,
}

#[derive(Debug, Clone)]
struct Artifact {
    path: String,
    sha256: String,
    size_bytes: u64,
}

struct GenesisFacts {
    subjects: Vec<Subject>,
    artifacts: Vec<Artifact>,
    commands: Value,
    environment_profile: String,
    network_mode: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Report<'a> {
    kind: &'a str,
    ok: bool,
    verifier_version: &'a str,
    compatibility_profile: String,
    bundle_sha256: String,
    policy_sha256: String,
    artifact_tree_sha256: String,
    artifact_tree_root: String,
    verified_attestations: usize,
    verified_signatures: usize,
    verified_artifacts: usize,
    verified_negative_controls: usize,
}

pub(crate) fn verify<'a>(args: &Args, version: &'a str) -> Result<Report<'a>, String> {
    validate_sha256(&args.policy_sha256, "--policy-sha256")?;
    let policy_bytes = read_bounded(&args.policy, HARD_INPUT_LIMIT, "trust policy")?;
    let policy_sha256 = sha256_hex(&policy_bytes);
    if policy_sha256 != args.policy_sha256 {
        return Err(format!(
            "policy SHA-256 mismatch: expected {} observed {policy_sha256}",
            args.policy_sha256
        ));
    }
    let policy_value = parse_unique(&policy_bytes, "trust policy")?;
    let policy: Policy = serde_json::from_value(policy_value)
        .map_err(|error| format!("trust policy schema violation: {error}"))?;
    validate_policy(&policy)?;
    let trusted_keys = trusted_keys(&policy)?;

    let tree_bytes = read_bounded(
        &args.artifact_tree,
        policy.limits.max_input_bytes,
        "artifact tree",
    )?;
    let tree_value = parse_unique(&tree_bytes, "artifact tree")?;
    let tree_canonical = canonical_bytes(&tree_value)?;
    let artifact_tree_sha256 = sha256_hex(&tree_canonical);
    if artifact_tree_sha256 != policy.artifact_tree_policy.manifest_digest.sha256 {
        return Err("artifact tree manifest digest does not match trust policy".to_owned());
    }
    let tree: ArtifactTree = serde_json::from_value(tree_value)
        .map_err(|error| format!("artifact tree schema violation: {error}"))?;
    let tree_entries = verify_artifact_tree(&tree, &policy, &args.artifact_root)?;

    let bundle_bytes = read_bounded(&args.bundle, policy.limits.max_input_bytes, "bundle")?;
    let bundle_sha256 = sha256_hex(&bundle_bytes);
    let bundle = parse_unique(&bundle_bytes, "bundle")?;
    let bundle_object = object(&bundle, "bundle")?;
    exact_keys(
        bundle_object,
        &["kind", "version", "profile", "attestations"],
        "bundle",
    )?;
    require_equal(
        string_field(bundle_object, "kind", "bundle")?,
        "genesis/evidence-bundle-v0.1",
        "bundle kind",
    )?;
    require_equal(
        string_field(bundle_object, "version", "bundle")?,
        "0.1",
        "bundle version",
    )?;
    let bundle_profile = string_field(bundle_object, "profile", "bundle")?;
    if !policy
        .accepted_bundle_profiles
        .iter()
        .any(|item| item == bundle_profile)
    {
        return Err(format!("bundle profile is not allowed: {bundle_profile}"));
    }
    let threshold = policy
        .signature_policy
        .thresholds_by_profile
        .get(bundle_profile)
        .copied()
        .ok_or_else(|| format!("signature threshold missing for profile {bundle_profile}"))?;
    if threshold == 0 {
        return Err("signature threshold must be positive".to_owned());
    }
    let attestations = array_field(bundle_object, "attestations", "bundle")?;
    if attestations.is_empty() || attestations.len() > policy.limits.max_attestations {
        return Err("attestation count is outside policy limits".to_owned());
    }

    let mut statements = BTreeMap::<String, Value>::new();
    let mut verified_signatures = 0usize;
    for (index, attestation) in attestations.iter().enumerate() {
        let label = format!("attestation[{index}]");
        let attestation = object(attestation, &label)?;
        exact_keys(attestation, &["mediaType", "statement", "envelope"], &label)?;
        require_equal(
            string_field(attestation, "mediaType", &label)?,
            PAYLOAD_TYPE,
            "attestation media type",
        )?;
        let statement = attestation
            .get("statement")
            .ok_or_else(|| format!("{label}.statement is missing"))?;
        let statement_object = object(statement, &format!("{label}.statement"))?;
        required_keys(
            statement_object,
            &["_type", "subject", "predicateType", "predicate"],
            &format!("{label}.statement"),
        )?;
        require_equal(
            string_field(statement_object, "_type", &format!("{label}.statement"))?,
            STATEMENT_TYPE,
            "statement type",
        )?;
        let predicate_type = string_field(
            statement_object,
            "predicateType",
            &format!("{label}.statement"),
        )?;
        if !policy
            .required_predicate_types
            .iter()
            .any(|item| item == predicate_type)
        {
            return Err(format!("predicate type is not allowed: {predicate_type}"));
        }
        if statements.contains_key(predicate_type) {
            return Err(format!("duplicate predicate type: {predicate_type}"));
        }
        let role = policy
            .signature_policy
            .predicate_roles
            .get(predicate_type)
            .ok_or_else(|| format!("no signature role for predicate {predicate_type}"))?;
        let envelope = attestation
            .get("envelope")
            .ok_or_else(|| format!("{label}.envelope is missing"))?;
        if envelope.is_null() {
            return Err(format!("{label} is unsigned for authenticated profile"));
        }
        verified_signatures += verify_envelope(
            envelope,
            statement,
            role,
            threshold,
            &trusted_keys,
            policy.limits.max_signatures_per_attestation,
            &label,
        )?;
        statements.insert(predicate_type.to_owned(), statement.clone());
    }
    for required in &policy.required_predicate_types {
        if !statements.contains_key(required) {
            return Err(format!("required predicate is missing: {required}"));
        }
    }
    if statements.len() != policy.required_predicate_types.len() {
        return Err("bundle contains predicates outside the compatibility profile".to_owned());
    }

    let genesis = statements
        .get(GENESIS_PREDICATE)
        .ok_or_else(|| "required Genesis predicate is missing".to_owned())?;
    let genesis_facts = validate_genesis_statement(genesis, bundle_profile, &policy)?;
    let genesis_digest = sha256_hex(&canonical_bytes(genesis)?);
    let slsa = statements
        .get(SLSA_PREDICATE)
        .ok_or_else(|| "required SLSA predicate is missing".to_owned())?;
    validate_slsa_statement(slsa, &genesis_facts, &genesis_digest, &policy)?;
    verify_subject_tree_coverage(&genesis_facts, &tree_entries)?;

    let verified_negative_controls = negative_control_count(genesis)?;
    Ok(Report {
        kind: "genesis/evidence-verification-result-v0.1",
        ok: true,
        verifier_version: version,
        compatibility_profile: policy.compatibility_profile.clone(),
        bundle_sha256,
        policy_sha256,
        artifact_tree_sha256,
        artifact_tree_root: tree.root_digest.sha256,
        verified_attestations: statements.len(),
        verified_signatures,
        verified_artifacts: tree_entries.len(),
        verified_negative_controls,
    })
}

fn validate_policy(policy: &Policy) -> Result<(), String> {
    require_equal(
        &policy.kind,
        "genesis/evidence-verifier-trust-policy-v0.1",
        "trust policy kind",
    )?;
    require_equal(&policy.version, "0.1", "trust policy version")?;
    require_equal(
        &policy.compatibility_profile,
        "genesis-evidence-v0.1+slsa-v1+dsse-v1",
        "compatibility profile",
    )?;
    require_sorted_unique(&policy.accepted_bundle_profiles, "accepted bundle profiles")?;
    require_sorted_unique(&policy.required_predicate_types, "required predicate types")?;
    if policy.required_predicate_types != [GENESIS_PREDICATE, SLSA_PREDICATE] {
        return Err("required predicate profile must contain Genesis and SLSA v1".to_owned());
    }
    if !policy.artifact_tree_policy.required {
        return Err("artifact tree policy must be required".to_owned());
    }
    require_equal(
        &policy.artifact_tree_policy.algorithm,
        "sha256-merkle-v0.1",
        "artifact tree algorithm",
    )?;
    validate_sha256(
        &policy.artifact_tree_policy.manifest_digest.sha256,
        "artifact tree manifest digest",
    )?;
    if !policy.source_policy.require_clean {
        return Err("independent verifier policy must require clean source".to_owned());
    }
    require_sorted_unique(
        &policy.source_policy.allowed_dirty_policies,
        "allowed dirty policies",
    )?;
    validate_uri(
        &policy.source_policy.expected_repository_uri,
        "expected source repository URI",
    )?;
    validate_git_revision(&policy.source_policy.expected_revision)?;
    validate_sha256(
        &policy.source_policy.expected_tree_digest.sha256,
        "expected source tree digest",
    )?;
    require_equal(
        &policy.network_policy.required_mode,
        "deny",
        "network policy mode",
    )?;
    if !policy.negative_control_policy.require_all_passed
        || policy.negative_control_policy.minimum_count == 0
    {
        return Err("negative-control policy must require at least one passing control".to_owned());
    }
    require_sorted_unique(
        &policy.compatibility.allowed_build_types,
        "allowed build types",
    )?;
    require_sorted_unique(
        &policy.compatibility.allowed_builder_ids,
        "allowed builder ids",
    )?;
    require_sorted_unique(
        &policy.compatibility.allowed_environment_profiles,
        "allowed environment profiles",
    )?;
    let limits = &policy.limits;
    if limits.max_input_bytes == 0
        || limits.max_input_bytes > HARD_INPUT_LIMIT
        || limits.max_attestations == 0
        || limits.max_signatures_per_attestation == 0
        || limits.max_subjects == 0
        || limits.max_artifacts == 0
        || limits.max_artifact_bytes == 0
    {
        return Err("trust policy limits are invalid or exceed verifier hard limits".to_owned());
    }
    Ok(())
}

fn trusted_keys(policy: &Policy) -> Result<BTreeMap<String, TrustedKey>, String> {
    if policy.signature_policy.trusted_keys.is_empty() {
        return Err("trust policy has no trusted keys".to_owned());
    }
    let mut result = BTreeMap::new();
    for key in &policy.signature_policy.trusted_keys {
        require_equal(&key.algorithm, "ed25519", "trusted key algorithm")?;
        let bytes = decode_base64(&key.public_key, "trusted public key")?;
        let key_bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| "trusted Ed25519 public key must be 32 bytes".to_owned())?;
        let expected_keyid = format!("sha256:{}", sha256_hex(&key_bytes));
        if key.keyid != expected_keyid {
            return Err("trusted keyid does not match public key".to_owned());
        }
        require_sorted_unique(&key.roles, "trusted key roles")?;
        let verifying_key = VerifyingKey::from_bytes(&key_bytes)
            .map_err(|_| "trusted Ed25519 public key is invalid".to_owned())?;
        let record = TrustedKey {
            keyid: key.keyid.clone(),
            key: verifying_key,
            roles: key.roles.iter().cloned().collect(),
        };
        if result.insert(key.keyid.clone(), record).is_some() {
            return Err(format!("duplicate trusted key: {}", key.keyid));
        }
    }
    Ok(result)
}

fn verify_envelope(
    value: &Value,
    statement: &Value,
    role: &str,
    threshold: usize,
    trusted_keys: &BTreeMap<String, TrustedKey>,
    max_signatures: usize,
    label: &str,
) -> Result<usize, String> {
    let envelope = object(value, &format!("{label}.envelope"))?;
    exact_keys(
        envelope,
        &["payloadType", "payload", "signatures"],
        &format!("{label}.envelope"),
    )?;
    let payload_type = string_field(envelope, "payloadType", "envelope")?;
    require_equal(payload_type, PAYLOAD_TYPE, "DSSE payload type")?;
    let payload = decode_base64(
        string_field(envelope, "payload", "envelope")?,
        "DSSE payload",
    )?;
    let expected_payload = canonical_bytes(statement)?;
    if payload != expected_payload {
        return Err("DSSE payload does not match statement".to_owned());
    }
    let pae = dsse_pae(payload_type, &payload);
    let signatures = array_field(envelope, "signatures", "envelope")?;
    if signatures.is_empty() || signatures.len() > max_signatures {
        return Err("signature count is outside policy limits".to_owned());
    }
    let mut seen = BTreeSet::new();
    let mut valid = 0usize;
    for (index, signature) in signatures.iter().enumerate() {
        let signature = object(signature, &format!("signature[{index}]"))?;
        exact_keys(signature, &["keyid", "sig"], &format!("signature[{index}]"))?;
        let keyid = string_field(signature, "keyid", "signature")?;
        if !seen.insert(keyid.to_owned()) {
            return Err(format!("duplicate signature keyid: {keyid}"));
        }
        let trusted = trusted_keys
            .get(keyid)
            .ok_or_else(|| format!("signature key is not trusted: {keyid}"))?;
        if trusted.keyid != keyid || !trusted.roles.contains(role) {
            return Err(format!("signature key is not authorized for role {role}"));
        }
        let signature_bytes = decode_base64(
            string_field(signature, "sig", "signature")?,
            "DSSE signature",
        )?;
        let signature = Signature::from_slice(&signature_bytes)
            .map_err(|_| "DSSE signature must be 64 bytes".to_owned())?;
        trusted
            .key
            .verify_strict(&pae, &signature)
            .map_err(|_| "signature verification failed".to_owned())?;
        valid += 1;
    }
    if valid < threshold {
        return Err(format!(
            "signature threshold not met: required {threshold} observed {valid}"
        ));
    }
    Ok(valid)
}

fn validate_genesis_statement(
    statement: &Value,
    bundle_profile: &str,
    policy: &Policy,
) -> Result<GenesisFacts, String> {
    let statement = object(statement, "Genesis statement")?;
    exact_keys(
        statement,
        &["_type", "subject", "predicateType", "predicate"],
        "Genesis statement",
    )?;
    let subjects = parse_subjects(
        statement,
        policy.limits.max_subjects,
        "Genesis statement",
        true,
    )?;
    let predicate = object_field(statement, "predicate", "Genesis statement")?;
    exact_keys(
        predicate,
        &[
            "kind",
            "version",
            "evidenceClass",
            "source",
            "toolchains",
            "environment",
            "networkPolicy",
            "commands",
            "negativeControls",
            "artifacts",
            "measurements",
            "verifier",
            "evidenceRefs",
        ],
        "Genesis predicate",
    )?;
    require_equal(
        string_field(predicate, "kind", "Genesis predicate")?,
        "genesis/evidence-predicate-v0.1",
        "Genesis predicate kind",
    )?;
    require_equal(
        string_field(predicate, "version", "Genesis predicate")?,
        "0.1",
        "Genesis predicate version",
    )?;
    require_equal(
        string_field(predicate, "evidenceClass", "Genesis predicate")?,
        bundle_profile,
        "evidence class",
    )?;

    let source = object_field(predicate, "source", "Genesis predicate")?;
    exact_keys(
        source,
        &[
            "vcs",
            "repositoryUri",
            "revision",
            "treeDigest",
            "dirty",
            "dirtyPolicy",
            "dirtyPathsDigest",
        ],
        "source",
    )?;
    require_equal(string_field(source, "vcs", "source")?, "git", "source VCS")?;
    let repository_uri = string_field(source, "repositoryUri", "source")?;
    validate_uri(repository_uri, "source repository URI")?;
    if repository_uri != policy.source_policy.expected_repository_uri {
        return Err("source repository URI does not match trust policy".to_owned());
    }
    let revision = string_field(source, "revision", "source")?;
    validate_git_revision(revision)?;
    if revision != policy.source_policy.expected_revision {
        return Err("source revision does not match trust policy".to_owned());
    }
    let tree_digest = digest_field(source, "treeDigest", "source")?;
    if tree_digest != policy.source_policy.expected_tree_digest.sha256 {
        return Err("source tree digest does not match trust policy".to_owned());
    }
    let dirty = bool_field(source, "dirty", "source")?;
    let dirty_policy = string_field(source, "dirtyPolicy", "source")?;
    if dirty {
        if dirty_policy == "reject" {
            return Err("dirty source requires an explicit non-reject policy".to_owned());
        }
        if !source.get("dirtyPathsDigest").is_some_and(Value::is_object) {
            return Err("dirty source requires dirtyPathsDigest".to_owned());
        }
        digest_field(source, "dirtyPathsDigest", "source")?;
    } else if dirty_policy != "reject" {
        return Err("clean source must use dirtyPolicy=reject".to_owned());
    } else if !source.get("dirtyPathsDigest").is_some_and(Value::is_null) {
        return Err("clean source requires dirtyPathsDigest=null".to_owned());
    }
    if policy.source_policy.require_clean && dirty {
        return Err("clean source required by trust policy".to_owned());
    }
    if !policy
        .source_policy
        .allowed_dirty_policies
        .iter()
        .any(|item| item == dirty_policy)
    {
        return Err(format!("dirty policy is not allowed: {dirty_policy}"));
    }

    let toolchains = array_field(predicate, "toolchains", "Genesis predicate")?;
    if toolchains.is_empty() || toolchains.len() > 128 {
        return Err("toolchain count is outside verifier limits".to_owned());
    }
    let mut tool_names = Vec::new();
    for (index, tool) in toolchains.iter().enumerate() {
        let tool = object(tool, &format!("toolchain[{index}]"))?;
        exact_keys(tool, &["name", "version", "artifact"], "toolchain")?;
        tool_names.push(string_field(tool, "name", "toolchain")?.to_owned());
        nonempty(
            string_field(tool, "version", "toolchain")?,
            "toolchain version",
        )?;
        validate_artifact_identity(
            object_field(tool, "artifact", "toolchain")?,
            "toolchain artifact",
        )?;
    }
    require_sorted_unique(&tool_names, "toolchains")?;

    let environment = object_field(predicate, "environment", "Genesis predicate")?;
    exact_keys(
        environment,
        &[
            "profile",
            "os",
            "architecture",
            "container",
            "declaredVariables",
        ],
        "environment",
    )?;
    let environment_profile = string_field(environment, "profile", "environment")?.to_owned();
    if !policy
        .compatibility
        .allowed_environment_profiles
        .iter()
        .any(|item| item == &environment_profile)
    {
        return Err(format!(
            "environment profile is not allowed: {environment_profile}"
        ));
    }
    nonempty(
        string_field(environment, "os", "environment")?,
        "environment OS",
    )?;
    nonempty(
        string_field(environment, "architecture", "environment")?,
        "environment architecture",
    )?;
    if let Some(container) = environment.get("container")
        && !container.is_null()
    {
        validate_artifact_identity(
            object(container, "environment container")?,
            "environment container",
        )?;
    }
    validate_string_array(
        array_field(environment, "declaredVariables", "environment")?,
        "declared environment variables",
        true,
    )?;

    let network = object_field(predicate, "networkPolicy", "Genesis predicate")?;
    exact_keys(network, &["mode", "inputs"], "network policy")?;
    let network_mode = string_field(network, "mode", "network policy")?.to_owned();
    require_equal(
        &network_mode,
        &policy.network_policy.required_mode,
        "network mode is not allowed",
    )?;
    let network_inputs = array_field(network, "inputs", "network policy")?;
    if network_mode == "deny" && !network_inputs.is_empty() {
        return Err("deny network mode contains declared inputs".to_owned());
    }
    for (index, input) in network_inputs.iter().enumerate() {
        let input = object(input, &format!("network input[{index}]"))?;
        exact_keys(input, &["uri", "digest", "purpose"], "network input")?;
        validate_uri(
            string_field(input, "uri", "network input")?,
            "network input URI",
        )?;
        digest_field(input, "digest", "network input")?;
        nonempty(
            string_field(input, "purpose", "network input")?,
            "network input purpose",
        )?;
    }

    let commands = predicate
        .get("commands")
        .ok_or_else(|| "Genesis predicate.commands is missing".to_owned())?
        .clone();
    validate_commands(&commands)?;

    let controls = array_field(predicate, "negativeControls", "Genesis predicate")?;
    if controls.len() < policy.negative_control_policy.minimum_count {
        return Err("negative-control count is below policy minimum".to_owned());
    }
    let mut control_ids = Vec::new();
    for (index, control) in controls.iter().enumerate() {
        let control = object(control, &format!("negative control[{index}]"))?;
        exact_keys(
            control,
            &["id", "expected", "observed", "passed", "artifact"],
            "negative control",
        )?;
        control_ids.push(string_field(control, "id", "negative control")?.to_owned());
        nonempty(
            string_field(control, "expected", "negative control")?,
            "negative expected",
        )?;
        nonempty(
            string_field(control, "observed", "negative control")?,
            "negative observed",
        )?;
        if policy.negative_control_policy.require_all_passed
            && !bool_field(control, "passed", "negative control")?
        {
            return Err("negative control did not pass".to_owned());
        }
        if let Some(artifact) = control.get("artifact")
            && !artifact.is_null()
        {
            validate_artifact_identity(
                object(artifact, "negative control artifact")?,
                "negative control artifact",
            )?;
        }
    }
    require_sorted_unique(&control_ids, "negative controls")?;

    let artifact_values = array_field(predicate, "artifacts", "Genesis predicate")?;
    if artifact_values.is_empty() || artifact_values.len() > policy.limits.max_artifacts {
        return Err("artifact count is outside policy limits".to_owned());
    }
    let mut artifacts = Vec::new();
    let mut artifact_names = Vec::new();
    for (index, artifact) in artifact_values.iter().enumerate() {
        let artifact = object(artifact, &format!("artifact[{index}]"))?;
        exact_keys(
            artifact,
            &["name", "path", "digest", "sizeBytes", "mediaType"],
            "artifact",
        )?;
        artifact_names.push(string_field(artifact, "name", "artifact")?.to_owned());
        let path = normalized_relative_path(string_field(artifact, "path", "artifact")?)?;
        let sha256 = digest_field(artifact, "digest", "artifact")?;
        let size_bytes = u64_field(artifact, "sizeBytes", "artifact")?;
        if size_bytes > policy.limits.max_artifact_bytes {
            return Err("artifact size exceeds policy limit".to_owned());
        }
        nonempty(
            string_field(artifact, "mediaType", "artifact")?,
            "artifact media type",
        )?;
        artifacts.push(Artifact {
            path,
            sha256,
            size_bytes,
        });
    }
    require_sorted_unique(&artifact_names, "artifacts")?;
    let artifact_subjects: Vec<Subject> = artifacts
        .iter()
        .map(|artifact| Subject {
            name: artifact.path.clone(),
            sha256: artifact.sha256.clone(),
        })
        .collect();
    if subjects != artifact_subjects {
        return Err("Genesis subjects do not exactly match predicate artifacts".to_owned());
    }

    validate_measurements(object_field(
        predicate,
        "measurements",
        "Genesis predicate",
    )?)?;
    validate_verifier(
        object_field(predicate, "verifier", "Genesis predicate")?,
        policy,
    )?;
    validate_evidence_refs(
        array_field(predicate, "evidenceRefs", "Genesis predicate")?,
        &policy.artifact_tree_policy.manifest_digest.sha256,
    )?;
    Ok(GenesisFacts {
        subjects,
        artifacts,
        commands,
        environment_profile,
        network_mode,
    })
}

fn validate_slsa_statement(
    statement: &Value,
    genesis: &GenesisFacts,
    genesis_digest: &str,
    policy: &Policy,
) -> Result<(), String> {
    let statement = object(statement, "SLSA statement")?;
    required_keys(
        statement,
        &["_type", "subject", "predicateType", "predicate"],
        "SLSA statement",
    )?;
    let subjects = parse_subjects(
        statement,
        policy.limits.max_subjects,
        "SLSA statement",
        false,
    )?;
    if subjects != genesis.subjects {
        return Err("Genesis and SLSA statement subjects differ".to_owned());
    }
    let predicate = object_field(statement, "predicate", "SLSA statement")?;
    required_keys(
        predicate,
        &["buildDefinition", "runDetails"],
        "SLSA predicate",
    )?;
    let definition = object_field(predicate, "buildDefinition", "SLSA predicate")?;
    required_keys(
        definition,
        &[
            "buildType",
            "externalParameters",
            "internalParameters",
            "resolvedDependencies",
        ],
        "SLSA build definition",
    )?;
    let build_type = string_field(definition, "buildType", "SLSA build definition")?;
    if !policy
        .compatibility
        .allowed_build_types
        .iter()
        .any(|item| item == build_type)
    {
        return Err(format!("build type is not allowed: {build_type}"));
    }
    let external = object_field(definition, "externalParameters", "SLSA build definition")?;
    exact_keys(
        external,
        &["commands", "evidenceProfile"],
        "SLSA external parameters",
    )?;
    require_equal(
        string_field(external, "evidenceProfile", "SLSA external parameters")?,
        "0.1",
        "SLSA evidence profile",
    )?;
    if external.get("commands") != Some(&genesis.commands) {
        return Err("SLSA commands do not match Genesis commands".to_owned());
    }
    let internal = object_field(definition, "internalParameters", "SLSA build definition")?;
    exact_keys(
        internal,
        &["environmentProfile", "networkMode"],
        "SLSA internal parameters",
    )?;
    require_equal(
        string_field(internal, "environmentProfile", "SLSA internal parameters")?,
        &genesis.environment_profile,
        "SLSA environment profile",
    )?;
    require_equal(
        string_field(internal, "networkMode", "SLSA internal parameters")?,
        &genesis.network_mode,
        "SLSA network mode",
    )?;
    let dependencies = array_field(definition, "resolvedDependencies", "SLSA build definition")?;
    if dependencies.is_empty() || dependencies.len() > policy.limits.max_artifacts {
        return Err("SLSA dependency count is outside policy limits".to_owned());
    }
    for (index, dependency) in dependencies.iter().enumerate() {
        validate_resource_descriptor(dependency, &format!("SLSA dependency[{index}]"))?;
    }

    let details = object_field(predicate, "runDetails", "SLSA predicate")?;
    required_keys(
        details,
        &["builder", "metadata", "byproducts"],
        "SLSA run details",
    )?;
    let builder = object_field(details, "builder", "SLSA run details")?;
    required_keys(
        builder,
        &["id", "version", "builderDependencies"],
        "SLSA builder",
    )?;
    let builder_id = string_field(builder, "id", "SLSA builder")?;
    if !policy
        .compatibility
        .allowed_builder_ids
        .iter()
        .any(|item| item == builder_id)
    {
        return Err(format!("builder id is not allowed: {builder_id}"));
    }
    if object_field(builder, "version", "SLSA builder")?.is_empty() {
        return Err("SLSA builder version must not be empty".to_owned());
    }
    for (index, dependency) in array_field(builder, "builderDependencies", "SLSA builder")?
        .iter()
        .enumerate()
    {
        validate_resource_descriptor(dependency, &format!("builder dependency[{index}]"))?;
    }
    let metadata = object_field(details, "metadata", "SLSA run details")?;
    required_keys(metadata, &["invocationId"], "SLSA metadata")?;
    require_equal(
        string_field(metadata, "invocationId", "SLSA metadata")?,
        &format!("urn:genesis:invocation:sha256:{genesis_digest}"),
        "SLSA invocation id",
    )?;
    let byproducts = array_field(details, "byproducts", "SLSA run details")?;
    if byproducts.is_empty() {
        return Err("SLSA byproducts must link the Genesis statement".to_owned());
    }
    let mut companion_found = false;
    for (index, byproduct) in byproducts.iter().enumerate() {
        let byproduct = object(byproduct, &format!("SLSA byproduct[{index}]"))?;
        validate_resource_descriptor_object(byproduct, &format!("SLSA byproduct[{index}]"))?;
        if digest_field(byproduct, "digest", "SLSA byproduct")? == genesis_digest
            && byproduct.get("mediaType").and_then(Value::as_str) == Some(PAYLOAD_TYPE)
        {
            companion_found = true;
        }
    }
    if !companion_found {
        return Err("SLSA byproducts do not bind the Genesis statement".to_owned());
    }
    Ok(())
}

fn validate_measurements(measurements: &Map<String, Value>) -> Result<(), String> {
    exact_keys(
        measurements,
        &["durationNs", "peakRssBytes", "diskDeltaBytes", "rawSamples"],
        "measurements",
    )?;
    u64_field(measurements, "durationNs", "measurements")?;
    u64_field(measurements, "peakRssBytes", "measurements")?;
    i64_field(measurements, "diskDeltaBytes", "measurements")?;
    let samples = array_field(measurements, "rawSamples", "measurements")?;
    if samples.is_empty() || samples.len() > 4096 {
        return Err("raw sample count is outside verifier limits".to_owned());
    }
    let mut metrics = Vec::new();
    for (index, sample) in samples.iter().enumerate() {
        let sample = object(sample, &format!("raw sample[{index}]"))?;
        exact_keys(sample, &["metric", "unit", "values"], "raw sample")?;
        metrics.push(string_field(sample, "metric", "raw sample")?.to_owned());
        let unit = string_field(sample, "unit", "raw sample")?;
        if !["ns", "bytes", "count", "basis-points"].contains(&unit) {
            return Err(format!("raw sample unit is unsupported: {unit}"));
        }
        let values = array_field(sample, "values", "raw sample")?;
        if values.is_empty() || values.len() > 1_000_000 {
            return Err("raw sample values are outside verifier limits".to_owned());
        }
        for value in values {
            if value.as_i64().is_none() && value.as_u64().is_none() {
                return Err("raw samples must contain integers".to_owned());
            }
        }
    }
    require_sorted_unique(&metrics, "raw sample metrics")
}

fn validate_verifier(verifier: &Map<String, Value>, policy: &Policy) -> Result<(), String> {
    exact_keys(verifier, &["name", "version", "artifact"], "verifier")?;
    let name = string_field(verifier, "name", "verifier")?;
    let version = string_field(verifier, "version", "verifier")?;
    let artifact = object_field(verifier, "artifact", "verifier")?;
    validate_artifact_identity(artifact, "verifier artifact")?;
    let uri = string_field(artifact, "uri", "verifier artifact")?;
    let digest = digest_field(artifact, "digest", "verifier artifact")?;
    if !policy
        .compatibility
        .allowed_verifiers
        .iter()
        .any(|allowed| {
            allowed.name == name
                && allowed.version == version
                && allowed.artifact_uri == uri
                && allowed.artifact_digest.sha256 == digest
        })
    {
        return Err("verifier is not allowed by compatibility policy".to_owned());
    }
    Ok(())
}

fn validate_evidence_refs(refs: &[Value], tree_digest: &str) -> Result<(), String> {
    if refs.len() > 4096 {
        return Err("evidence reference count exceeds verifier limit".to_owned());
    }
    let mut uris = Vec::new();
    let mut tree_found = false;
    for (index, reference) in refs.iter().enumerate() {
        let reference = object(reference, &format!("evidence ref[{index}]"))?;
        exact_keys(
            reference,
            &["kind", "uri", "digest", "mediaType"],
            "evidence ref",
        )?;
        let kind = string_field(reference, "kind", "evidence ref")?;
        let uri = string_field(reference, "uri", "evidence ref")?;
        validate_uri(uri, "evidence reference URI")?;
        uris.push(uri.to_owned());
        let digest = digest_field(reference, "digest", "evidence ref")?;
        nonempty(
            string_field(reference, "mediaType", "evidence ref")?,
            "evidence media type",
        )?;
        if kind == "genesis/artifact-hash-tree-v0.1" && digest == tree_digest {
            tree_found = true;
        }
    }
    require_sorted_unique(&uris, "evidence references")?;
    if !tree_found {
        return Err("Genesis evidence does not bind the required artifact tree".to_owned());
    }
    Ok(())
}

fn validate_commands(value: &Value) -> Result<(), String> {
    let commands = value
        .as_array()
        .ok_or_else(|| "commands must be an array".to_owned())?;
    if commands.is_empty() || commands.len() > 4096 {
        return Err("command count is outside verifier limits".to_owned());
    }
    for (index, command) in commands.iter().enumerate() {
        let command = object(command, &format!("command[{index}]"))?;
        exact_keys(
            command,
            &["argv", "cwd", "declaredEnvironment", "exitCode"],
            "command",
        )?;
        let argv = array_field(command, "argv", "command")?;
        if argv.is_empty() || argv.len() > 4096 {
            return Err("command argv count is outside verifier limits".to_owned());
        }
        for argument in argv {
            nonempty(
                argument
                    .as_str()
                    .ok_or_else(|| "command argv must contain strings".to_owned())?,
                "command argument",
            )?;
        }
        normalized_relative_path(string_field(command, "cwd", "command")?)?;
        validate_string_array(
            array_field(command, "declaredEnvironment", "command")?,
            "command environment",
            true,
        )?;
        i64_field(command, "exitCode", "command")?;
    }
    Ok(())
}

fn parse_subjects(
    statement: &Map<String, Value>,
    limit: usize,
    label: &str,
    strict: bool,
) -> Result<Vec<Subject>, String> {
    let values = array_field(statement, "subject", label)?;
    if values.is_empty() || values.len() > limit {
        return Err(format!("{label} subject count is outside policy limits"));
    }
    let mut result = Vec::new();
    for (index, subject) in values.iter().enumerate() {
        let subject = object(subject, &format!("{label}.subject[{index}]"))?;
        if strict {
            exact_keys(subject, &["name", "digest"], "statement subject")?;
        } else {
            required_keys(subject, &["name", "digest"], "statement subject")?;
        }
        result.push(Subject {
            name: normalized_relative_path(string_field(subject, "name", "statement subject")?)?,
            sha256: digest_field(subject, "digest", "statement subject")?,
        });
    }
    let sorted: BTreeSet<_> = result.iter().cloned().collect();
    if sorted.len() != result.len() || sorted.iter().cloned().collect::<Vec<_>>() != result {
        return Err(format!("{label} subjects must be sorted and unique"));
    }
    Ok(result)
}

fn verify_artifact_tree(
    tree: &ArtifactTree,
    policy: &Policy,
    artifact_root: &Path,
) -> Result<BTreeMap<String, Artifact>, String> {
    require_equal(
        &tree.kind,
        "genesis/artifact-hash-tree-v0.1",
        "artifact tree kind",
    )?;
    require_equal(&tree.version, "0.1", "artifact tree version")?;
    require_equal(
        &tree.algorithm,
        &policy.artifact_tree_policy.algorithm,
        "artifact tree algorithm",
    )?;
    validate_sha256(&tree.root_digest.sha256, "artifact tree root")?;
    if tree.entries.is_empty() || tree.entries.len() > policy.limits.max_artifacts {
        return Err("artifact tree entry count is outside policy limits".to_owned());
    }
    let root_metadata = fs::symlink_metadata(artifact_root)
        .map_err(|error| format!("artifact root is unavailable: {error}"))?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err("artifact root must be a non-symlink directory".to_owned());
    }
    let canonical_root = artifact_root
        .canonicalize()
        .map_err(|error| format!("artifact root canonicalization failed: {error}"))?;
    let mut entries = BTreeMap::new();
    let mut leaves = Vec::new();
    let mut observed_paths = Vec::new();
    for entry in &tree.entries {
        require_equal(&entry.entry_type, "file", "artifact tree entry type")?;
        let path = normalized_relative_path(&entry.path)?;
        validate_sha256(&entry.digest.sha256, "artifact tree entry digest")?;
        if entry.size_bytes > policy.limits.max_artifact_bytes {
            return Err("artifact tree entry exceeds policy size limit".to_owned());
        }
        observed_paths.push(path.clone());
        reject_symlink_components(artifact_root, &path)?;
        let candidate = artifact_root.join(&path);
        let canonical_candidate = candidate
            .canonicalize()
            .map_err(|error| format!("artifact is unavailable: {path}: {error}"))?;
        if !canonical_candidate.starts_with(&canonical_root) {
            return Err(format!("artifact escapes artifact root: {path}"));
        }
        let metadata = fs::metadata(&canonical_candidate)
            .map_err(|error| format!("artifact metadata failed: {path}: {error}"))?;
        if !metadata.is_file() || metadata.len() != entry.size_bytes {
            return Err(format!("artifact size mismatch: {path}"));
        }
        let observed_digest = hash_file(&canonical_candidate, entry.size_bytes)?;
        if observed_digest != entry.digest.sha256 {
            return Err(format!("artifact digest mismatch: {path}"));
        }
        let artifact = Artifact {
            path: path.clone(),
            sha256: entry.digest.sha256.clone(),
            size_bytes: entry.size_bytes,
        };
        if entries.insert(path.clone(), artifact).is_some() {
            return Err(format!("duplicate artifact tree path: {path}"));
        }
        leaves.push(hash_tree_leaf(
            &path,
            entry.size_bytes,
            &entry.digest.sha256,
        )?);
    }
    require_sorted_unique(&observed_paths, "artifact tree entries")?;
    let root = hash_tree_root(leaves)?;
    if root != tree.root_digest.sha256 {
        return Err(format!(
            "artifact Merkle root mismatch: expected {} observed {root}",
            tree.root_digest.sha256
        ));
    }
    Ok(entries)
}

fn verify_subject_tree_coverage(
    genesis: &GenesisFacts,
    tree: &BTreeMap<String, Artifact>,
) -> Result<(), String> {
    if genesis.artifacts.len() != tree.len() {
        return Err("artifact tree does not exactly cover Genesis artifacts".to_owned());
    }
    for artifact in &genesis.artifacts {
        let entry = tree
            .get(&artifact.path)
            .ok_or_else(|| format!("artifact tree is missing subject: {}", artifact.path))?;
        if entry.sha256 != artifact.sha256 || entry.size_bytes != artifact.size_bytes {
            return Err(format!("artifact tree subject mismatch: {}", artifact.path));
        }
    }
    Ok(())
}

fn hash_tree_leaf(path: &str, size: u64, digest: &str) -> Result<Vec<u8>, String> {
    let digest = decode_hex_32(digest, "artifact digest")?;
    let path = path.as_bytes();
    let mut hasher = Sha256::new();
    hasher.update(b"GenesisCodeHashTreeLeafv0.1\0");
    hasher.update((path.len() as u64).to_be_bytes());
    hasher.update(path);
    hasher.update(size.to_be_bytes());
    hasher.update(digest);
    Ok(hasher.finalize().to_vec())
}

fn hash_tree_root(mut nodes: Vec<Vec<u8>>) -> Result<String, String> {
    if nodes.is_empty() {
        return Err("artifact tree cannot be empty".to_owned());
    }
    while nodes.len() > 1 {
        let mut next = Vec::with_capacity(nodes.len().div_ceil(2));
        let mut chunks = nodes.chunks_exact(2);
        for pair in &mut chunks {
            let mut hasher = Sha256::new();
            hasher.update(b"GenesisCodeHashTreeNodev0.1\0");
            hasher.update(&pair[0]);
            hasher.update(&pair[1]);
            next.push(hasher.finalize().to_vec());
        }
        if let Some(last) = chunks.remainder().first() {
            next.push(last.clone());
        }
        nodes = next;
    }
    Ok(hex_bytes(&nodes[0]))
}

fn reject_symlink_components(root: &Path, relative: &str) -> Result<(), String> {
    let mut current = PathBuf::from(root);
    for component in Path::new(relative).components() {
        let Component::Normal(component) = component else {
            return Err(format!("artifact path is not normalized: {relative}"));
        };
        current.push(component);
        let metadata = fs::symlink_metadata(&current).map_err(|error| {
            format!("artifact path component is unavailable: {relative}: {error}")
        })?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "symbolic links are forbidden in artifact paths: {relative}"
            ));
        }
    }
    Ok(())
}

fn hash_file(path: &Path, expected_size: u64) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| format!("artifact open failed: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("artifact read failed: {error}"))?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or_else(|| "artifact size overflow".to_owned())?;
        if total > expected_size {
            return Err("artifact grew while being verified".to_owned());
        }
        hasher.update(&buffer[..read]);
    }
    if total != expected_size {
        return Err("artifact changed size while being verified".to_owned());
    }
    Ok(hex_bytes(&hasher.finalize()))
}

fn negative_control_count(statement: &Value) -> Result<usize, String> {
    let statement = object(statement, "Genesis statement")?;
    let predicate = object_field(statement, "predicate", "Genesis statement")?;
    Ok(array_field(predicate, "negativeControls", "Genesis predicate")?.len())
}

fn validate_artifact_identity(value: &Map<String, Value>, label: &str) -> Result<(), String> {
    exact_keys(value, &["uri", "digest"], label)?;
    validate_uri(string_field(value, "uri", label)?, label)?;
    digest_field(value, "digest", label)?;
    Ok(())
}

fn validate_resource_descriptor(value: &Value, label: &str) -> Result<(), String> {
    validate_resource_descriptor_object(object(value, label)?, label)
}

fn validate_resource_descriptor_object(
    value: &Map<String, Value>,
    label: &str,
) -> Result<(), String> {
    digest_field(value, "digest", label)?;
    if let Some(uri) = value.get("uri") {
        validate_uri(
            uri.as_str()
                .ok_or_else(|| format!("{label}.uri must be a string"))?,
            label,
        )?;
    }
    if let Some(name) = value.get("name") {
        nonempty(
            name.as_str()
                .ok_or_else(|| format!("{label}.name must be a string"))?,
            label,
        )?;
    }
    if let Some(media_type) = value.get("mediaType") {
        nonempty(
            media_type
                .as_str()
                .ok_or_else(|| format!("{label}.mediaType must be a string"))?,
            label,
        )?;
    }
    if let Some(annotations) = value.get("annotations")
        && !annotations.is_object()
    {
        return Err(format!("{label}.annotations must be an object"));
    }
    Ok(())
}

fn validate_string_array(values: &[Value], label: &str, sorted: bool) -> Result<(), String> {
    let mut strings = Vec::new();
    for value in values {
        strings.push(
            value
                .as_str()
                .ok_or_else(|| format!("{label} must contain strings"))?
                .to_owned(),
        );
    }
    if sorted {
        require_sorted_unique(&strings, label)?;
    }
    Ok(())
}

fn normalized_relative_path(value: &str) -> Result<String, String> {
    if value == "." {
        return Ok(value.to_owned());
    }
    if value.is_empty() || value.contains('\\') || value.contains("//") {
        return Err(format!("normalized relative path required: {value}"));
    }
    if value.len() >= 2 && value.as_bytes()[1] == b':' {
        return Err(format!("drive-qualified path is forbidden: {value}"));
    }
    let path = Path::new(value);
    if path.is_absolute() {
        return Err(format!("absolute path is forbidden: {value}"));
    }
    for component in path.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(format!("normalized relative path required: {value}"));
        }
    }
    Ok(value.to_owned())
}

fn validate_uri(value: &str, label: &str) -> Result<(), String> {
    let Some((scheme, rest)) = value.split_once(':') else {
        return Err(format!("{label} must be an absolute URI"));
    };
    if scheme.is_empty()
        || !scheme
            .chars()
            .enumerate()
            .all(|(index, ch)| ch.is_ascii_alphanumeric() || (index > 0 && "+-.".contains(ch)))
        || rest.is_empty()
        || scheme.eq_ignore_ascii_case("file")
    {
        return Err(format!("{label} must be a non-file absolute URI"));
    }
    Ok(())
}

fn validate_git_revision(value: &str) -> Result<(), String> {
    if !matches!(value.len(), 40 | 64)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("Git revision must be 40 or 64 lowercase hexadecimal characters".to_owned());
    }
    Ok(())
}

fn read_bounded(path: &Path, limit: u64, label: &str) -> Result<Vec<u8>, String> {
    let metadata =
        fs::metadata(path).map_err(|error| format!("{label} is unavailable: {error}"))?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(format!("{label} exceeds the input limit or is not a file"));
    }
    fs::read(path).map_err(|error| format!("{label} read failed: {error}"))
}

fn object<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))
}

fn object_field<'a>(
    value: &'a Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<&'a Map<String, Value>, String> {
    object(
        value
            .get(field)
            .ok_or_else(|| format!("{label}.{field} is missing"))?,
        &format!("{label}.{field}"),
    )
}

fn array_field<'a>(
    value: &'a Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<&'a [Value], String> {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("{label}.{field} must be an array"))
}

fn string_field<'a>(
    value: &'a Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<&'a str, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{label}.{field} must be a string"))
}

fn bool_field(value: &Map<String, Value>, field: &str, label: &str) -> Result<bool, String> {
    value
        .get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("{label}.{field} must be boolean"))
}

fn u64_field(value: &Map<String, Value>, field: &str, label: &str) -> Result<u64, String> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{label}.{field} must be a nonnegative integer"))
}

fn i64_field(value: &Map<String, Value>, field: &str, label: &str) -> Result<i64, String> {
    value
        .get(field)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("{label}.{field} must be an integer"))
}

fn digest_field(value: &Map<String, Value>, field: &str, label: &str) -> Result<String, String> {
    let digest = object_field(value, field, label)?;
    exact_keys(digest, &["sha256"], &format!("{label}.{field}"))?;
    let digest = string_field(digest, "sha256", &format!("{label}.{field}"))?;
    validate_sha256(digest, &format!("{label}.{field}.sha256"))?;
    Ok(digest.to_owned())
}

fn exact_keys(value: &Map<String, Value>, expected: &[&str], label: &str) -> Result<(), String> {
    let expected: BTreeSet<&str> = expected.iter().copied().collect();
    let observed: BTreeSet<&str> = value.keys().map(String::as_str).collect();
    let missing: Vec<_> = expected.difference(&observed).copied().collect();
    let unknown: Vec<_> = observed.difference(&expected).copied().collect();
    if !missing.is_empty() {
        return Err(format!("{label} missing fields: {}", missing.join(", ")));
    }
    if !unknown.is_empty() {
        return Err(format!(
            "{label} contains unknown fields: {}",
            unknown.join(", ")
        ));
    }
    Ok(())
}

fn required_keys(value: &Map<String, Value>, expected: &[&str], label: &str) -> Result<(), String> {
    let observed: BTreeSet<&str> = value.keys().map(String::as_str).collect();
    let missing: Vec<_> = expected
        .iter()
        .copied()
        .filter(|key| !observed.contains(key))
        .collect();
    if !missing.is_empty() {
        return Err(format!("{label} missing fields: {}", missing.join(", ")));
    }
    Ok(())
}

fn require_sorted_unique(values: &[String], label: &str) -> Result<(), String> {
    let sorted: BTreeSet<_> = values.iter().cloned().collect();
    if sorted.len() != values.len() || sorted.iter().cloned().collect::<Vec<_>>() != values {
        return Err(format!("{label} must be sorted and unique"));
    }
    Ok(())
}

fn require_equal(value: &str, expected: &str, label: &str) -> Result<(), String> {
    if value != expected {
        return Err(format!("{label}: expected {expected}, observed {value}"));
    }
    Ok(())
}

fn nonempty(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    Ok(())
}

fn validate_sha256(value: &str, label: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(format!(
            "{label} must be 64 lowercase hexadecimal characters"
        ));
    }
    Ok(())
}

fn decode_base64(value: &str, label: &str) -> Result<Vec<u8>, String> {
    let decoded = BASE64
        .decode(value)
        .map_err(|_| format!("{label} is not valid standard base64"))?;
    if BASE64.encode(&decoded) != value {
        return Err(format!("{label} is not canonical padded base64"));
    }
    Ok(decoded)
}

fn dsse_pae(payload_type: &str, payload: &[u8]) -> Vec<u8> {
    format!(
        "DSSEv1 {} {} {} ",
        payload_type.len(),
        payload_type,
        payload.len()
    )
    .into_bytes()
    .into_iter()
    .chain(payload.iter().copied())
    .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex_bytes(&Sha256::digest(bytes))
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn decode_hex_32(value: &str, label: &str) -> Result<[u8; 32], String> {
    validate_sha256(value, label)?;
    let mut output = [0u8; 32];
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_nibble(chunk[0]).ok_or_else(|| format!("{label} is invalid hexadecimal"))?;
        let low = hex_nibble(chunk[1]).ok_or_else(|| format!("{label} is invalid hexadecimal"))?;
        output[index] = (high << 4) | low;
    }
    Ok(output)
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BASE64, GENESIS_PREDICATE, SLSA_PREDICATE, canonical_bytes, dsse_pae, hash_tree_leaf,
        hash_tree_root, normalized_relative_path, sha256_hex, validate_uri, verify,
    };
    use crate::Args;
    use base64::Engine;
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
        args: Args,
    }

    impl Fixture {
        fn new(case_id: &str) -> Self {
            let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
            let suffix = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "genesis-evidence-verifier-{}-{suffix}-{case_id}",
                std::process::id()
            ));
            if root.exists() {
                fs::remove_dir_all(&root).expect("remove stale verifier fixture");
            }
            fs::create_dir_all(root.join("evidence/artifact"))
                .expect("create verifier fixture root");
            copy(
                &repository.join("docs/program/evidence/GENESIS_EVIDENCE_BUNDLE_v0.1.json"),
                &root.join("bundle.json"),
            );
            copy(
                &repository.join("docs/program/evidence/GENESIS_EVIDENCE_ARTIFACT_TREE_v0.1.json"),
                &root.join("tree.json"),
            );
            copy(
                &repository.join("policies/evidence_verifier_trust_v0.1.json"),
                &root.join("policy.json"),
            );
            copy(
                &repository.join("docs/program/evidence/artifact/genesis-example.bin"),
                &root.join("evidence/artifact/genesis-example.bin"),
            );
            let policy_sha256 =
                sha256_hex(&fs::read(root.join("policy.json")).expect("read fixture policy"));
            Self {
                args: Args {
                    bundle: root.join("bundle.json"),
                    policy: root.join("policy.json"),
                    policy_sha256,
                    artifact_tree: root.join("tree.json"),
                    artifact_root: root.join("evidence"),
                },
                root,
            }
        }

        fn bundle(&self) -> Value {
            read_json(&self.args.bundle)
        }

        fn tree(&self) -> Value {
            read_json(&self.args.artifact_tree)
        }

        fn policy(&self) -> Value {
            read_json(&self.args.policy)
        }

        fn write_bundle(&self, value: &Value, resign: bool) {
            let mut value = value.clone();
            if resign {
                resign_bundle(&mut value);
            }
            write_json(&self.args.bundle, &value);
        }

        fn write_tree(&mut self, value: &Value, rebind_policy: bool) {
            write_json(&self.args.artifact_tree, value);
            if rebind_policy {
                let mut policy = self.policy();
                policy["artifactTreePolicy"]["manifestDigest"]["sha256"] =
                    Value::String(sha256_hex(&canonical_bytes(value).expect("canonical tree")));
                self.write_policy(&policy, true);
            }
        }

        fn write_policy(&mut self, value: &Value, rebind_cli: bool) {
            write_json(&self.args.policy, value);
            if rebind_cli {
                self.args.policy_sha256 =
                    sha256_hex(&fs::read(&self.args.policy).expect("read rebound fixture policy"));
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn copy(source: &Path, destination: &Path) {
        fs::copy(source, destination).expect("copy verifier fixture");
    }

    fn read_json(path: &Path) -> Value {
        serde_json::from_slice(&fs::read(path).expect("read verifier fixture"))
            .expect("parse verifier fixture")
    }

    fn write_json(path: &Path, value: &Value) {
        let mut bytes = serde_json::to_vec_pretty(value).expect("encode verifier fixture");
        bytes.push(b'\n');
        fs::write(path, bytes).expect("write verifier fixture");
    }

    fn resign_bundle(bundle: &mut Value) {
        let seed = Sha256::digest(b"GenesisCode public evidence profile test key v0.1");
        let seed: [u8; 32] = seed.into();
        let key = SigningKey::from_bytes(&seed);
        for attestation in bundle["attestations"]
            .as_array_mut()
            .expect("fixture attestations")
        {
            let statement = &attestation["statement"];
            let payload = canonical_bytes(statement).expect("canonical fixture statement");
            let signature = key.sign(&dsse_pae(super::PAYLOAD_TYPE, &payload));
            attestation["envelope"]["payload"] = Value::String(BASE64.encode(&payload));
            attestation["envelope"]["signatures"][0]["sig"] =
                Value::String(BASE64.encode(signature.to_bytes()));
        }
    }

    fn mutate_statement<'a>(bundle: &'a mut Value, predicate_type: &str) -> &'a mut Value {
        bundle["attestations"]
            .as_array_mut()
            .expect("fixture attestations")
            .iter_mut()
            .find(|attestation| {
                attestation["statement"]["predicateType"].as_str() == Some(predicate_type)
            })
            .map(|attestation| &mut attestation["statement"])
            .expect("fixture predicate")
    }

    fn expected_cases() -> Vec<(String, String)> {
        let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let catalog: Value = serde_json::from_slice(
            &fs::read(repository.join(
                "docs/program/evidence/GENESIS_EVIDENCE_VERIFIER_NEGATIVE_VECTORS_v0.1.json",
            ))
            .expect("read negative catalog"),
        )
        .expect("parse negative catalog");
        catalog["cases"]
            .as_array()
            .expect("negative cases")
            .iter()
            .map(|case| {
                (
                    case["id"].as_str().expect("case id").to_owned(),
                    case["expectedDiagnostic"]
                        .as_str()
                        .expect("case diagnostic")
                        .to_owned(),
                )
            })
            .collect()
    }

    fn apply_case(fixture: &mut Fixture, case_id: &str) {
        match case_id {
            "artifact-content-substitution" => {
                fs::write(
                    fixture.root.join("evidence/artifact/genesis-example.bin"),
                    b"XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX",
                )
                .expect("substitute fixture artifact");
            }
            "artifact-path-traversal" => {
                let mut tree = fixture.tree();
                tree["entries"][0]["path"] = Value::String("../genesis-example.bin".to_owned());
                fixture.write_tree(&tree, true);
            }
            "artifact-path-alias" => {
                let mut tree = fixture.tree();
                tree["entries"][0]["path"] =
                    Value::String("./artifact/genesis-example.bin".to_owned());
                fixture.write_tree(&tree, true);
            }
            "artifact-tree-root-substitution" => {
                let mut tree = fixture.tree();
                tree["rootDigest"]["sha256"] = Value::String("f".repeat(64));
                fixture.write_tree(&tree, true);
            }
            "artifact-type-substitution" => {
                let path = fixture.root.join("evidence/artifact/genesis-example.bin");
                fs::remove_file(&path).expect("remove artifact fixture");
                fs::create_dir(&path).expect("replace fixture with directory");
            }
            "bundle-duplicate-key" => {
                let text = fs::read_to_string(&fixture.args.bundle).expect("read bundle text");
                let text = text.replacen(
                    "\"version\": \"0.1\"",
                    "\"version\": \"0.1\", \"version\": \"0.1\"",
                    1,
                );
                fs::write(&fixture.args.bundle, text).expect("write duplicate-key bundle");
            }
            "bundle-profile-unsupported" => {
                let mut bundle = fixture.bundle();
                bundle["profile"] = Value::String("E4".to_owned());
                fixture.write_bundle(&bundle, false);
            }
            "bundle-missing-field" => {
                let mut bundle = fixture.bundle();
                bundle
                    .as_object_mut()
                    .expect("bundle object")
                    .remove("kind");
                fixture.write_bundle(&bundle, false);
            }
            "bundle-version-unsupported" => {
                let mut bundle = fixture.bundle();
                bundle["version"] = Value::String("9.9".to_owned());
                fixture.write_bundle(&bundle, false);
            }
            "dirty-policy-missing" => {
                let mut bundle = fixture.bundle();
                let source =
                    mutate_statement(&mut bundle, GENESIS_PREDICATE)["predicate"]["source"]
                        .as_object_mut()
                        .expect("source object");
                source.insert("dirty".to_owned(), Value::Bool(true));
                source.remove("dirtyPolicy");
                source.insert(
                    "dirtyPathsDigest".to_owned(),
                    json!({"sha256": "f".repeat(64)}),
                );
                fixture.write_bundle(&bundle, true);
            }
            "dirty-paths-digest-missing" => {
                let mut bundle = fixture.bundle();
                let source =
                    &mut mutate_statement(&mut bundle, GENESIS_PREDICATE)["predicate"]["source"];
                source["dirty"] = Value::Bool(true);
                source["dirtyPolicy"] = Value::String("allow-declared".to_owned());
                source["dirtyPathsDigest"] = Value::Null;
                fixture.write_bundle(&bundle, true);
            }
            "dirty-source" => {
                let mut bundle = fixture.bundle();
                let statement = mutate_statement(&mut bundle, GENESIS_PREDICATE);
                statement["predicate"]["source"]["dirty"] = Value::Bool(true);
                statement["predicate"]["source"]["dirtyPolicy"] =
                    Value::String("allow-declared".to_owned());
                statement["predicate"]["source"]["dirtyPathsDigest"] =
                    json!({"sha256": "f".repeat(64)});
                fixture.write_bundle(&bundle, true);
            }
            "failed-negative-control" => {
                let mut bundle = fixture.bundle();
                mutate_statement(&mut bundle, GENESIS_PREDICATE)["predicate"]["negativeControls"]
                    [0]["passed"] = Value::Bool(false);
                fixture.write_bundle(&bundle, true);
            }
            "float-number" => {
                let text = fs::read_to_string(&fixture.args.bundle).expect("read bundle text");
                let text = text.replacen("\"durationNs\": 12500000", "\"durationNs\": 1.5", 1);
                fs::write(&fixture.args.bundle, text).expect("write float bundle");
            }
            "forged-signature" => {
                let mut bundle = fixture.bundle();
                bundle["attestations"][0]["envelope"]["signatures"][0]["sig"] =
                    Value::String(BASE64.encode([0u8; 64]));
                fixture.write_bundle(&bundle, false);
            }
            "hash-tree-policy-substitution" => {
                let mut tree = fixture.tree();
                tree["entries"][0]["digest"]["sha256"] = Value::String("f".repeat(64));
                fixture.write_tree(&tree, false);
            }
            "missing-slsa-companion" => {
                let mut bundle = fixture.bundle();
                bundle["attestations"]
                    .as_array_mut()
                    .expect("fixture attestations")
                    .pop();
                fixture.write_bundle(&bundle, false);
            }
            "network-policy-bypass" => {
                let mut bundle = fixture.bundle();
                let network = &mut mutate_statement(&mut bundle, GENESIS_PREDICATE)["predicate"]["networkPolicy"];
                network["mode"] = Value::String("declared-only".to_owned());
                network["inputs"] = json!([{
                    "uri": "https://example.invalid/input",
                    "digest": {"sha256": "f".repeat(64)},
                    "purpose": "negative fixture"
                }]);
                fixture.write_bundle(&bundle, true);
            }
            "payload-substitution" => {
                let mut bundle = fixture.bundle();
                bundle["attestations"][0]["envelope"]["payload"] =
                    Value::String(BASE64.encode(b"{}"));
                fixture.write_bundle(&bundle, false);
            }
            "policy-self-trust-injection" => {
                let mut policy = fixture.policy();
                let key = policy["signaturePolicy"]["trustedKeys"][0].clone();
                policy["signaturePolicy"]["trustedKeys"]
                    .as_array_mut()
                    .expect("trusted keys")
                    .push(key);
                fixture.write_policy(&policy, false);
            }
            "signature-threshold-bypass" => {
                let mut policy = fixture.policy();
                policy["signaturePolicy"]["thresholdsByProfile"]["E3"] = Value::Number(2.into());
                fixture.write_policy(&policy, true);
            }
            "slsa-build-type-substitution" => {
                let mut bundle = fixture.bundle();
                mutate_statement(&mut bundle, SLSA_PREDICATE)["predicate"]["buildDefinition"]["buildType"] =
                    Value::String("https://example.invalid/build/v1".to_owned());
                fixture.write_bundle(&bundle, true);
            }
            "slsa-builder-substitution" => {
                let mut bundle = fixture.bundle();
                mutate_statement(&mut bundle, SLSA_PREDICATE)["predicate"]["runDetails"]["builder"]
                    ["id"] = Value::String("https://example.invalid/builder/v1".to_owned());
                fixture.write_bundle(&bundle, true);
            }
            "statement-subject-divergence" => {
                let mut bundle = fixture.bundle();
                mutate_statement(&mut bundle, SLSA_PREDICATE)["subject"][0]["digest"]["sha256"] =
                    Value::String("f".repeat(64));
                fixture.write_bundle(&bundle, true);
            }
            "source-repository-stale" => {
                let mut bundle = fixture.bundle();
                mutate_statement(&mut bundle, GENESIS_PREDICATE)["predicate"]["source"]["repositoryUri"] =
                    Value::String("https://example.invalid/stale.git".to_owned());
                fixture.write_bundle(&bundle, true);
            }
            "source-revision-stale" => {
                let mut bundle = fixture.bundle();
                mutate_statement(&mut bundle, GENESIS_PREDICATE)["predicate"]["source"]["revision"] =
                    Value::String("f".repeat(40));
                fixture.write_bundle(&bundle, true);
            }
            "source-tree-stale" => {
                let mut bundle = fixture.bundle();
                mutate_statement(&mut bundle, GENESIS_PREDICATE)["predicate"]["source"]["treeDigest"]
                    ["sha256"] = Value::String("f".repeat(64));
                fixture.write_bundle(&bundle, true);
            }
            "unsupported-predicate" => {
                let mut bundle = fixture.bundle();
                mutate_statement(&mut bundle, SLSA_PREDICATE)["predicateType"] =
                    Value::String("https://example.invalid/predicate/v1".to_owned());
                fixture.write_bundle(&bundle, true);
            }
            "untrusted-key" => {
                let mut bundle = fixture.bundle();
                bundle["attestations"][0]["envelope"]["signatures"][0]["keyid"] =
                    Value::String(format!("sha256:{}", "f".repeat(64)));
                fixture.write_bundle(&bundle, false);
            }
            "verifier-identity-substitution" => {
                let mut bundle = fixture.bundle();
                mutate_statement(&mut bundle, GENESIS_PREDICATE)["predicate"]["verifier"]["version"] =
                    Value::String("9.9.9".to_owned());
                fixture.write_bundle(&bundle, true);
            }
            other => panic!("unimplemented negative vector: {other}"),
        }
    }

    #[test]
    fn fixture_merkle_leaf_matches_profile() {
        let leaf = hash_tree_leaf(
            "artifact/genesis-example.bin",
            34,
            "d8b1c4946a814e874c0eb109674d5e4fd49f84f3317794dc69d24b267a4aa72d",
        )
        .expect("valid fixture leaf");
        let root = hash_tree_root(vec![leaf]).expect("valid fixture tree");
        assert_eq!(
            root,
            "e3bcf8b287b3f3a1735de56a8516aebaaad84276f3ff42b2b63339c1249c519a"
        );
    }

    #[test]
    fn rejects_path_aliases_and_file_uris() {
        assert!(normalized_relative_path("../artifact").is_err());
        assert!(normalized_relative_path("./artifact").is_err());
        assert!(normalized_relative_path("/artifact").is_err());
        assert!(normalized_relative_path("C:/artifact").is_err());
        assert!(validate_uri("file:///tmp/artifact", "fixture").is_err());
    }

    #[test]
    fn published_adversarial_vectors_fail_closed_at_expected_boundary() {
        let cases = expected_cases();
        assert_eq!(cases.len(), 30);
        for (case_id, expected_diagnostic) in cases {
            let mut fixture = Fixture::new(&case_id);
            apply_case(&mut fixture, &case_id);
            let error = match verify(&fixture.args, "0.1.0-test") {
                Ok(_) => panic!("negative vector was accepted: {case_id}"),
                Err(error) => error,
            };
            assert!(
                error.contains(&expected_diagnostic),
                "negative vector {case_id} failed at wrong boundary: expected `{expected_diagnostic}`, observed `{error}`"
            );
        }
    }

    #[test]
    fn accepts_authenticated_monotonic_slsa_extensions() {
        let fixture = Fixture::new("slsa-monotonic-extensions");
        let mut bundle = fixture.bundle();
        let statement = mutate_statement(&mut bundle, SLSA_PREDICATE);
        statement["futureStatementField"] = json!({"ignored": true});
        statement["subject"][0]["annotations"] = json!({"future": "value"});
        statement["predicate"]["futurePredicateField"] = json!({"ignored": true});
        statement["predicate"]["buildDefinition"]["futureDefinitionField"] =
            json!({"ignored": true});
        statement["predicate"]["buildDefinition"]["resolvedDependencies"][0]["downloadLocation"] =
            json!("https://example.invalid/source.tar");
        statement["predicate"]["runDetails"]["futureRunField"] = json!({"ignored": true});
        statement["predicate"]["runDetails"]["builder"]["futureBuilderField"] =
            json!({"ignored": true});
        statement["predicate"]["runDetails"]["metadata"]["startedOn"] =
            json!("2026-07-10T00:00:00Z");
        fixture.write_bundle(&bundle, true);
        let result = verify(&fixture.args, "0.1.0-test");
        assert!(result.is_ok(), "monotonic extension rejected: {result:?}");
    }
}
