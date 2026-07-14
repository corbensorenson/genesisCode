#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -ne 1 ]]; then
  echo "usage: $0 <report-output>" >&2
  exit 2
fi

REPORT_OUT="$1"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "domain-starter-registry-bootstrap" \
  root-host

GENESIS_BIN="${GENESIS_BIN:-$CARGO_TARGET_DIR/debug/genesis}"
MANIFEST_PATH="${GENESIS_DOMAIN_STARTER_REGISTRY_MANIFEST_PATH:-$ROOT_DIR/docs/skill_pack/write_genesiscode_v1/manifest.json}"

cargo build -p gc_cli --bin genesis >/dev/null

tmp_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

publisher="$tmp_dir/publisher"
consumer="$tmp_dir/consumer"
remote_dir="$tmp_dir/remote-registry"
mkdir -p "$publisher" "$consumer" "$remote_dir"
publisher_real="$(cd "$publisher" && pwd -P)"
consumer_real="$(cd "$consumer" && pwd -P)"

artifact="$tmp_dir/selfhost_toolchain.gc"
if [[ -f "$ROOT_DIR/selfhost/toolchain.gc" ]]; then
  cp "$ROOT_DIR/selfhost/toolchain.gc" "$artifact"
else
  "$GENESIS_BIN" selfhost-artifact --out "$artifact" >/dev/null
fi

remote="file://$remote_dir/"
remote_allow="${remote}v1/"

g() {
  "$GENESIS_BIN" --selfhost-only --selfhost-artifact "$artifact" "$@"
}

store_put() {
  local dir="$1"
  local caps="$2"
  local src="$3"
  local file="$4"
  printf '%s\n' "$src" >"$dir/$file"
  local out
  out="$(g store --caps "$caps" put --input "$dir/$file")"
  printf '%s' "$out" | tr -d '\n'
}

cat >"$publisher/caps.toml" <<EOF_CAPS
allow = [
  "core/store::put",
  "core/store::get",
  "core/store::has",
  "core/refs::get",
  "core/refs::set",
  "core/sync::push",
  "core/pkg-low::load-package",
  "core/pkg-low::snapshot",
  "core/gpk-low::export",
  "core/pkg-low::publish"
]

[store]
dir = "$publisher_real/.genesis/store"

[refs]
path = "$publisher_real/.genesis/refs.gc"

[op."core/pkg-low::snapshot"]
base_dir = "$publisher_real"

[op."core/gpk-low::export"]
base_dir = "$publisher_real"
create_dirs = true

[op."core/pkg-low::publish"]
base_dir = "$publisher_real"
remote_allow = ["$remote_allow"]
wasi_network_profile = "local"

[op."core/sync::push"]
remote_allow = ["$remote_allow"]
wasi_network_profile = "local"
EOF_CAPS

cat >"$consumer/caps.toml" <<EOF_CAPS
allow = [
  "core/store::put",
  "core/store::get",
  "core/store::has",
  "core/sync::pull",
  "core/pkg-low::init",
  "core/pkg-low::add",
  "core/pkg-low::lock",
  "core/pkg-low::install",
  "core/pkg-low::save-lock",
  "core/pkg-low::load-lock",
  "core/pkg-low::verify",
  "core/pkg-low::info"
]

[store]
dir = "$consumer_real/.genesis/store"
remote = "$remote"
remote_allow = ["$remote_allow"]

[op."core/sync::pull"]
remote_allow = ["$remote_allow"]
wasi_network_profile = "local"

[op."core/pkg-low::init"]
base_dir = "$consumer_real"
create_dirs = true

[op."core/pkg-low::add"]
base_dir = "$consumer_real"

[op."core/pkg-low::lock"]
base_dir = "$consumer_real"
create_dirs = true

[op."core/pkg-low::install"]
base_dir = "$consumer_real"

[op."core/pkg-low::save-lock"]
base_dir = "$consumer_real"
create_dirs = true

[op."core/pkg-low::load-lock"]
base_dir = "$consumer_real"

[op."core/pkg-low::verify"]
base_dir = "$consumer_real"

[op."core/pkg-low::info"]
base_dir = "$consumer_real"
EOF_CAPS

g keygen --out "$publisher/signing_key.toml" >/dev/null

policy_h="$(store_put "$publisher" "$publisher/caps.toml" '{
  :type :vcs/policy
  :v 1
  :name "policy:starter-bundles-v0.1"
  :refs {:frozen-prefixes []}
  :classes {
    :dev  {:patterns ["refs/**/heads/*"] :exclude ["refs/**/heads/main"] :required-obligations [core/obligation::unit-tests] :require-signatures false}
    :main {:patterns ["refs/**/heads/main"] :required-obligations [core/obligation::unit-tests] :require-signatures false}
    :tags {:patterns ["refs/**/tags/*"] :required-obligations [core/obligation::unit-tests] :require-signatures false}
  }
}' policy.gc)"

patch_h="$(store_put "$publisher" "$publisher/caps.toml" '{:type :vcs/patch :v 1 :ops []}' patch.gc)"

mkdir -p "$publisher/starters" "$publisher/bundles"
report_rows="$tmp_dir/starter_rows.tsv"
: >"$report_rows"

starters=(
  "starter-service|service|core/kit/service::status-v1"
  "starter-graphics|graphics|core/kit/graphics::frame-loop-v1"
  "starter-gpu-compute|gpu_compute|core/kit/gpu::compute-pipeline-v1"
  "starter-gpu-non-graphics|gpu_non_graphics|core/kit/gpu::non-graphics-pipeline-v1"
  "starter-package-publish-sync|package_publish_sync|core/kit/package::publish-sync-v1"
  "starter-deployment-targets|deployment_targets|core/kit/deploy::bundle-targets-v1"
  "starter-failure-recovery|failure_recovery|core/kit/recovery::fault-injection-v1"
  "starter-performance-triage|performance_triage|core/kit/perf::triage-run-v1"
  "starter-assurance|assurance|core/kit/assurance::profile-pack-v1"
  "starter-plugin-ffi|plugin_ffi|core/kit/plugin::invoke-v1"
  "starter-xr-runtime|xr_runtime|core/kit/xr::runtime-session-v1"
  "starter-xr-productization|xr_productization|core/kit/xr::productization-kit-v1"
  "starter-durable-data|durable_data|core/kit/data::durable-pipeline-v1"
  "starter-process-lifecycle|process_lifecycle|core/kit/process::lifecycle-v1"
  "starter-filesystem|filesystem|core/kit/fs::workspace-io-v1"
  "starter-network-process|network_process|core/kit/net::service-process-v1"
  "starter-raw-network-sockets|raw_network_sockets|core/kit/net::raw-socket-v1"
  "starter-inbound-server|inbound_server|core/kit/net::inbound-server-v1"
  "starter-time-control|time_control|core/kit/time::deterministic-control-v1"
  "starter-multi-agent-orchestration|multi_agent_orchestration|core/kit/agent::orchestration-v1"
  "starter-realtime-collaboration|realtime_collaboration|core/kit/collab::realtime-session-v1"
  "starter-backend-topology|backend_topology|core/kit/backend::topology-v1"
  "starter-browser-runtime|browser_runtime|core/kit/browser::runtime-v1"
  "starter-ml-data-engineering|ml_data_engineering|core/kit/ml::data-engineering-v1"
  "starter-complex-ui-app-stacks|complex_ui_app_stacks|core/kit/ui::complex-stack-v1"
  "starter-hardware-device-integration|hardware_device_integration|core/kit/hardware::device-integration-v1"
  "starter-security-auth-services|security_auth_services|core/kit/security::auth-service-v1"
)

for spec in "${starters[@]}"; do
  IFS='|' read -r name domain entry <<<"$spec"

  pkg_dir="$publisher/starters/$name"
  mkdir -p "$pkg_dir"

  cat >"$pkg_dir/lib.gc" <<EOF_MOD
(def starter/meta::record {
  :package "$name"
  :domain "$domain"
  :entry "$entry"
  :ai-first true
})
starter/meta::record
EOF_MOD

  cat >"$pkg_dir/package.toml" <<EOF_PKG
name = "$name"
version = "0.1.0"
obligations = []
dependencies = []

[[modules]]
path = "lib.gc"
EOF_PKG

  acceptance_h="$(g pack --pkg "$pkg_dir/package.toml" | tr -d '\n')"
  signature_h="$(g sign --pkg "$pkg_dir/package.toml" --key "$publisher/signing_key.toml" --acceptance "$acceptance_h" --signatures "$publisher/signatures.gc" | tr -d '\n')"

  snapshot_h="$(
    cd "$publisher"
    g pkg --caps "$publisher/caps.toml" snapshot --pkg "starters/$name/package.toml" | tr -d '\n'
  )"
  evidence_h="$(store_put "$publisher" "$publisher/caps.toml" "{:type :vcs/evidence :v 1 :kind :unit-tests :inputs [] :outputs [] :data nil}" "$name.evidence.gc")"
  commit_h="$(store_put "$publisher" "$publisher/caps.toml" "{
    :type :vcs/commit
    :v 1
    :parents []
    :target {:kind :package :name \"$name\"}
    :base nil
    :patch \"$patch_h\"
    :result \"$snapshot_h\"
    :obligations [core/obligation::unit-tests]
    :evidence [\"$evidence_h\"]
    :attestations []
    :message \"publish $name v0.1.0\"
  }" "$name.commit.gc")"

  ref_name="refs/pkgs/$name/tags/v0.1.0"
  g refs --caps "$publisher/caps.toml" set "$ref_name" "$commit_h" --policy "$policy_h" >/dev/null
  published_h="$(g pkg --caps "$publisher/caps.toml" publish --remote "$remote" --ref "$ref_name" --policy "$policy_h" --expected-old nil --commit "$commit_h" | tr -d '\n')"
  if [[ "$published_h" != "$commit_h" ]]; then
    echo "domain-starter-registry-bootstrap: publish mismatch name=$name expected=$commit_h got=$published_h" >&2
    exit 1
  fi

  bundle_out="$publisher/bundles/$name-v0.1.0.gpk"
  (
    cd "$publisher"
    g pkg --caps "$publisher/caps.toml" export --snapshot "$snapshot_h" --out "bundles/$name-v0.1.0.gpk" >/dev/null
  )
  if [[ ! -f "$bundle_out" ]]; then
    echo "domain-starter-registry-bootstrap: missing exported bundle for $name" >&2
    exit 1
  fi

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$name" "$domain" "$entry" "$acceptance_h" "$signature_h" "$snapshot_h" "$commit_h" "$bundle_out" \
    >>"$report_rows"
done

(
  cd "$consumer"
  g gcpm --caps "$consumer/caps.toml" new \
    --workspace starter-bootstrap \
    --policy policy:default-v0.1 \
    --registry-default "$remote" >/dev/null
)

for spec in "${starters[@]}"; do
  IFS='|' read -r name _domain _entry <<<"$spec"
  commit_h="$(awk -F'\t' -v n="$name" '$1 == n {print $7}' "$report_rows" | tail -n1)"
  if [[ -z "$commit_h" ]]; then
    echo "domain-starter-registry-bootstrap: missing commit hash for $name" >&2
    exit 1
  fi
  (
    cd "$consumer"
    g gcpm --caps "$consumer/caps.toml" add "$name@commit:$commit_h" --registry default >/dev/null
    g sync --caps "$consumer/caps.toml" pull --remote "$remote" --root "$commit_h" >/dev/null
  )
done

(
  cd "$consumer"
  g gcpm --caps "$consumer/caps.toml" lock --strict >/dev/null
  g gcpm --caps "$consumer/caps.toml" install --strict --frozen >/dev/null
)

consumer_list="$tmp_dir/consumer_list.txt"
(
  cd "$consumer"
  g gcpm --caps "$consumer/caps.toml" list >"$consumer_list"
)
python3 - "$consumer_list" "$report_rows" <<'PY'
import pathlib
import re
import sys

list_text = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")
expected = {}
for raw in pathlib.Path(sys.argv[2]).read_text(encoding="utf-8").splitlines():
    if not raw.strip():
        continue
    fields = raw.split("\t")
    expected[fields[0]] = fields[6]

observed = {}
for block in re.findall(r"\{[^{}]*\}", list_text):
    name_match = re.search(r':name\s+"([^"]+)"', block)
    commit_match = re.search(r':commit\s+"([^"]+)"', block)
    if name_match and commit_match:
        observed[name_match.group(1)] = commit_match.group(1)

missing = sorted(set(expected) - set(observed))
mismatched = sorted(
    name for name, commit in expected.items() if observed.get(name) != commit
)
unexpected = sorted(set(observed) - set(expected))
if missing or mismatched or unexpected:
    raise SystemExit(
        "domain-starter-registry-bootstrap: gcpm list lock mismatch "
        f"missing={missing} mismatched={mismatched} unexpected={unexpected}"
    )
print(f"domain-starter-registry-bootstrap: gcpm list verified entries={len(expected)}")
PY

info_samples="$tmp_dir/info_samples.tsv"
: >"$info_samples"
sample_indices=(0 $(( ${#starters[@]} / 2 )) $(( ${#starters[@]} - 1 )))
for sample_idx in "${sample_indices[@]}"; do
  IFS='|' read -r name _domain _entry <<<"${starters[$sample_idx]}"
  commit_h="$(awk -F'\t' -v n="$name" '$1 == n {print $7}' "$report_rows" | tail -n1)"
  info="$(
    cd "$consumer"
    g gcpm --caps "$consumer/caps.toml" info "$name" | tr -d '\n'
  )"
  if [[ "$info" != *"$commit_h"* ]]; then
    echo "domain-starter-registry-bootstrap: gcpm info missing locked commit for $name" >&2
    exit 1
  fi
  printf '%s\t%s\n' "$name" "$commit_h" >>"$info_samples"
done

python3 - "$report_rows" "$info_samples" "$REPORT_OUT" "$MANIFEST_PATH" "$ROOT_DIR" <<'PY'
import hashlib
import json
import pathlib
import sys

rows_path = pathlib.Path(sys.argv[1])
info_samples_path = pathlib.Path(sys.argv[2])
report_path = pathlib.Path(sys.argv[3])
manifest_path = pathlib.Path(sys.argv[4])
root = pathlib.Path(sys.argv[5]).resolve()

if not manifest_path.is_file():
    raise SystemExit(
        f"domain-starter-registry-bootstrap: missing manifest: {manifest_path}"
    )

starters = []
for raw in rows_path.read_text(encoding="utf-8").splitlines():
    if not raw.strip():
        continue
    name, domain, entry, acceptance_h, signature_h, snapshot_h, commit_h, bundle = raw.split("\t")
    bundle_path = pathlib.Path(bundle)
    if not bundle_path.is_file():
        raise SystemExit(
            f"domain-starter-registry-bootstrap: missing bundle while rendering evidence: {name}"
        )
    bundle_bytes = bundle_path.read_bytes()
    starters.append(
        {
            "name": name,
            "domain": domain,
            "entry": entry,
            "acceptance_hash": acceptance_h,
            "signature_hash": signature_h,
            "snapshot_hash": snapshot_h,
            "commit_hash": commit_h,
            "bundle_filename": bundle_path.name,
            "bundle_sha256": hashlib.sha256(bundle_bytes).hexdigest(),
            "bundle_size_bytes": len(bundle_bytes),
        }
    )

manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
manifest_resolved = manifest_path.resolve()
try:
    manifest_identity = manifest_resolved.relative_to(root).as_posix()
except ValueError:
    manifest_identity = f"external/{manifest_resolved.name}"
manifest_sha256 = hashlib.sha256(manifest_path.read_bytes()).hexdigest()
required_domains = manifest.get("distribution_requirements", {}).get("required_recipe_domains")
if not isinstance(required_domains, list) or not all(
    isinstance(domain, str) and domain for domain in required_domains
):
    raise SystemExit(
        "domain-starter-registry-bootstrap: manifest missing distribution_requirements.required_recipe_domains"
    )

starter_domain_set = sorted({starter["domain"] for starter in starters})
required_domain_set = sorted(set(required_domains))
missing_domains = sorted(set(required_domain_set) - set(starter_domain_set))
unexpected_domains = sorted(set(starter_domain_set) - set(required_domain_set))
coverage_ok = not missing_domains
info_samples = []
for raw in info_samples_path.read_text(encoding="utf-8").splitlines():
    if raw.strip():
        name, commit = raw.split("\t")
        info_samples.append({"name": name, "commit_hash": commit})

report = {
    "kind": "genesis/domain-starter-registry-bootstrap-v0.1",
    "ok": coverage_ok,
    "registry_transport": "file-v1",
    "registry_scope": "isolated-temporary",
    "manifest_path": manifest_identity,
    "manifest_sha256": manifest_sha256,
    "starter_count": len(starters),
    "required_domain_count": len(required_domain_set),
    "required_domains": required_domain_set,
    "starter_domains": starter_domain_set,
    "missing_domains": missing_domains,
    "unexpected_domains": unexpected_domains,
    "starters": starters,
    "consumer_lock_path": "genesis.lock",
    "consumer_list_verified": True,
    "consumer_list_entry_count": len(starters),
    "consumer_info_samples": info_samples,
}
report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
if not coverage_ok:
    raise SystemExit(
        "domain-starter-registry-bootstrap: missing required domains: "
        + ", ".join(missing_domains)
    )
print(
    "domain-starter-registry-bootstrap: ok "
    f"starters={len(starters)} required_domains={len(required_domain_set)} transport=file-v1 report={report_path}"
)
PY
