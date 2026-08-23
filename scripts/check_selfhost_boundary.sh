#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

SEMANTIC_TOKEN_REGEX='parse_module\(|canonicalize_module\(|print_module\(|hash_module\(|eval_module\(|eval_term\('
MODE="${SELFHOST_BOUNDARY_MODE:-diff}"

is_allowed_path() {
  local p="$1"
  case "$p" in
    crates/gc_coreform/*) return 0 ;;
    crates/gc_kernel/*) return 0 ;;
    crates/gc_prelude/*) return 0 ;;
    crates/gc_opt/*) return 0 ;;
    crates/gc_types/*) return 0 ;;
    crates/gc_patches/*) return 0 ;;
    crates/gc_obligations/*) return 0 ;;
    crates/gc_cli_driver/src/cmd_*.rs) return 0 ;;
    crates/gc_cli_driver/src/selfhost_bridge.rs) return 0 ;;
    crates/gc_cli_driver/src/pkg_self_opt.rs) return 0 ;;
    crates/gc_cli_driver/src/kernel_exec.rs) return 0 ;;
    crates/gc_effects/src/lib.rs) return 0 ;;
    crates/gc_effects/src/runner*.rs) return 0 ;;
    crates/*/src/tests.rs) return 0 ;;
    crates/*/src/tests_*.rs) return 0 ;;
    crates/gc_cli/src/main.rs) return 0 ;;
    crates/gc_wasi_cli/src/main.rs) return 0 ;;
    crates/gc_wasm/src/lib.rs) return 0 ;;
    crates/gc_wasm/src/runtime.rs) return 0 ;;
    crates/*/tests/*) return 0 ;;
    *) return 1 ;;
  esac
}

usage() {
  cat <<'EOF'
Usage: scripts/check_selfhost_boundary.sh [--diff|--strict]

Modes:
  --diff    inspect only semantic tokens added in diff lines (default)
  --strict  inspect full Rust source tree for semantic tokens in non-approved files
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --diff)
      MODE="diff"
      shift
      ;;
    --strict)
      MODE="strict"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "selfhost-boundary: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ "$MODE" != "diff" && "$MODE" != "strict" ]]; then
  echo "selfhost-boundary: invalid mode '$MODE' (expected diff|strict)" >&2
  exit 2
fi

# Validate the normative trust model even when diff mode has no Rust files to scan.
python3 scripts/lib/stage0_trust_contract.py \
  --root "$ROOT_DIR" \
  --contract docs/spec/STAGE0_TRUST_CONTRACT_v0.1.json \
  --schema docs/spec/STAGE0_TRUST_CONTRACT_v0.1.schema.json \
  --spec docs/spec/SELF_HOST_BOUNDARY.md \
  --self-test

# Enforce the exact non-dev Cargo and source closure bound to the trust model.
python3 scripts/lib/stage0_dependency_boundary.py \
  --root "$ROOT_DIR" \
  --contract docs/spec/STAGE0_DEPENDENCY_BOUNDARY_v0.1.json \
  --schema docs/spec/STAGE0_DEPENDENCY_BOUNDARY_v0.1.schema.json \
  --spec docs/spec/STAGE0_DEPENDENCY_BOUNDARY_v0.1.md \
  --stage0 docs/spec/STAGE0_TRUST_CONTRACT_v0.1.json \
  --self-test

# H-level claims are cumulative per-decision predicates, not routing labels.
python3 scripts/lib/selfhost_closure_levels.py \
  --root "$ROOT_DIR" \
  --contract docs/spec/SELFHOST_CLOSURE_LEVELS_v0.1.json \
  --schema docs/spec/SELFHOST_CLOSURE_LEVELS_v0.1.schema.json \
  --spec docs/spec/SELFHOST_CLOSURE_LEVELS_v0.1.md \
  --stage0 docs/spec/STAGE0_TRUST_CONTRACT_v0.1.json \
  --self-test

# R4.2.a production frontend authority is closed, identity-bound, and fallback-free.
python3 scripts/lib/selfhost_frontend_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_frontend_authority_v0.1.json \
  --schema docs/spec/SELFHOST_FRONTEND_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.b type/effect authority is closed, dependency-isolated, and mutation-checked.
python3 scripts/lib/selfhost_typecheck_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_typecheck_authority_v0.1.json \
  --schema docs/spec/SELFHOST_TYPECHECK_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.c patch authority is H2 only while source, route, and final production
# graph custody remain independently checked and the parity oracle stays opt-in.
python3 scripts/lib/selfhost_patch_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_patch_authority_v0.1.json \
  --schema docs/spec/SELFHOST_PATCH_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.d policy aliases are independently checked without promoting the other
# obligation, effect-policy, replay, signing, or evidence decisions in this task.
python3 scripts/lib/selfhost_policy_alias_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_policy_alias_authority_v0.1.json \
  --schema docs/spec/SELFHOST_POLICY_ALIAS_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.d effect-policy composition is H2 for its exact decision domain: neutral
# host transport is bounded and no production Rust semantic oracle remains.
python3 scripts/lib/selfhost_effect_policy_composition.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_effect_policy_composition_v0.1.json \
  --schema docs/spec/SELFHOST_EFFECT_POLICY_COMPOSITION_v0.1.schema.json \
  --self-test

# R4.2.d exact replay semantics are H2: production consumes the request-bound
# GenesisCode verdict and the legacy Rust checker is parity-harness-only.
python3 scripts/lib/selfhost_replay_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_replay_authority_v0.1.json \
  --schema docs/spec/SELFHOST_REPLAY_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.d signing semantics are H2: GenesisCode owns every signed byte and
# persisted semantic artifact while bounded cryptography and custody stay host mechanisms.
python3 scripts/lib/selfhost_signing_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_signing_authority_v0.1.json \
  --schema docs/spec/SELFHOST_SIGNING_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.d evidence verification is H2: GenesisCode owns package, policy,
# transparency, and GenesisBench DSSE verdicts; host mechanisms cannot accept.
python3 scripts/lib/selfhost_evidence_verify_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_evidence_verify_authority_v0.1.json \
  --schema docs/spec/SELFHOST_EVIDENCE_VERIFY_AUTHORITY_v0.1.schema.json

# R4.2.e store authority is partial: GenesisCode exclusively decides put
# admission, canonical bytes, budgets, and identity while the ledger remains H0.
python3 scripts/lib/selfhost_store_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_store_authority_v0.1.json \
  --schema docs/spec/SELFHOST_STORE_AUTHORITY_v0.1.schema.json \
  --artifact selfhost/toolchain.gc \
  --self-test

# R4.2.e artifact GC is H2: GenesisCode owns pins, roots, edges, dead/reclaim,
# report, and purge selection while Rust retains only bounded exact mechanisms.
python3 scripts/lib/selfhost_gc_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_gc_authority_v0.1.json \
  --schema docs/spec/SELFHOST_GC_AUTHORITY_v0.1.schema.json \
  --artifact selfhost/toolchain.gc \
  --self-test

# R4.2.e commit authority is partial: GenesisCode constructs and validates
# native commit objects while internal package/registry/VCS consumers remain H0.
python3 scripts/lib/selfhost_commit_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_commit_authority_v0.1.json \
  --schema docs/spec/SELFHOST_COMMIT_AUTHORITY_v0.1.schema.json \
  --artifact selfhost/toolchain.gc \
  --self-test

# R4.2.e refs authority is partial: GenesisCode owns direct lookup, list, CAS,
# update, and delete decisions; policy admission and bulk sync/GPK updates remain H0.
python3 scripts/lib/selfhost_refs_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_refs_authority_v0.1.json \
  --schema docs/spec/SELFHOST_REFS_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.e package lock writing is partial: GenesisCode owns canonical save-lock
# normalization, bytes, and identity; parsing, resolution, and persistence remain H0.
python3 scripts/lib/selfhost_pkg_lock_write_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_pkg_lock_write_authority_v0.1.json \
  --schema docs/spec/SELFHOST_PKG_LOCK_WRITE_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.e package lock reading is partial: GenesisCode owns the typed public
# lock result; bounded transport and the generic Rust TOML codec remain H0.
python3 scripts/lib/selfhost_pkg_lock_read_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_pkg_lock_read_authority_v0.1.json \
  --schema docs/spec/SELFHOST_PKG_LOCK_READ_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.e internal package lock modeling is partial: GenesisCode owns the
# complete typed model used by resolution routes; generic TOML and mechanisms remain H0.
python3 scripts/lib/selfhost_pkg_lock_model_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_pkg_lock_model_authority_v0.1.json \
  --schema docs/spec/SELFHOST_PKG_LOCK_MODEL_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.e direct lock operations are partial: GenesisCode owns init/add mutation,
# canonical bytes, and list projection; TOML transport and persistence remain H0.
python3 scripts/lib/selfhost_pkg_lock_ops_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_pkg_lock_ops_authority_v0.1.json \
  --schema docs/spec/SELFHOST_PKG_LOCK_OPS_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.e bridge object authority is partial: GenesisCode owns the complete
# object/signing graph; storage, Ed25519, lock, and registry mechanisms remain H0.
python3 scripts/lib/selfhost_pkg_bridge_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_pkg_bridge_authority_v0.1.json \
  --schema docs/spec/SELFHOST_PKG_BRIDGE_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.e direct package snapshot authority is partial: GenesisCode owns exact
# module/snapshot objects and identities; package frontend transport remains H0.
python3 scripts/lib/selfhost_pkg_snapshot_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_pkg_snapshot_authority_v0.1.json \
  --schema docs/spec/SELFHOST_PKG_SNAPSHOT_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.e package identity is partial: GenesisCode owns canonical requirement
# fingerprints; selector parsing, graph solving, and registry mechanisms remain H0.
python3 scripts/lib/selfhost_pkg_resolution_identity_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_pkg_resolution_identity_authority_v0.1.json \
  --schema docs/spec/SELFHOST_PKG_RESOLUTION_IDENTITY_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.e resolution planning is partial: GenesisCode owns selector/strategy,
# semver-policy, and update-admission decisions; host resolution mechanisms remain H0.
python3 scripts/lib/selfhost_pkg_resolution_plan_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_pkg_resolution_plan_authority_v0.1.json \
  --schema docs/spec/SELFHOST_PKG_RESOLUTION_PLAN_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.e resolution workflow authority is partial: GenesisCode owns lock/update
# causal decisions and exact evidence objects; host resolver mechanisms remain H0.
python3 scripts/lib/selfhost_pkg_resolution_workflow_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_pkg_resolution_workflow_authority_v0.1.json \
  --schema docs/spec/SELFHOST_PKG_RESOLUTION_WORKFLOW_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.e install authority is partial: GenesisCode owns admission, ordered
# dependency planning, observation binding, verdict, and provenance; hydration remains H0.
python3 scripts/lib/selfhost_pkg_install_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_pkg_install_authority_v0.1.json \
  --schema docs/spec/SELFHOST_PKG_INSTALL_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.e verify authority is partial: GenesisCode owns ordered verification,
# terminal disposition, accounting, and report semantics around bounded host mechanisms.
python3 scripts/lib/selfhost_pkg_verify_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_pkg_verify_authority_v0.1.json \
  --schema docs/spec/SELFHOST_PKG_VERIFY_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.e scaffold authority is partial: GenesisCode owns normalization,
# archetype/render/identity/report decisions; bounded host persistence remains H0.
python3 scripts/lib/selfhost_pkg_scaffold_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_pkg_scaffold_authority_v0.1.json \
  --schema docs/spec/SELFHOST_PKG_SCAFFOLD_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.e workspace-new authority is partial: GenesisCode owns member,
# document, identity, and report semantics; bounded host persistence remains H0.
python3 scripts/lib/selfhost_pkg_workspace_new_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_pkg_workspace_new_authority_v0.1.json \
  --schema docs/spec/SELFHOST_PKG_WORKSPACE_NEW_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.e workspace-remove authority is partial: GenesisCode owns exact lock
# mutation, canonical writer composition, identity, and report decisions.
python3 scripts/lib/selfhost_pkg_workspace_remove_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_pkg_workspace_remove_authority_v0.1.json \
  --schema docs/spec/SELFHOST_PKG_WORKSPACE_REMOVE_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.e workspace-migrate authority is partial: GenesisCode owns migration
# selection, model, rendering, identity, and report decisions around bounded host IO.
python3 scripts/lib/selfhost_pkg_workspace_migrate_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_pkg_workspace_migrate_authority_v0.1.json \
  --schema docs/spec/SELFHOST_PKG_WORKSPACE_MIGRATE_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.e workspace-manifest authority is partial: GenesisCode owns structural
# admission and normalized typed workspace configuration; TOML syntax remains host.
python3 scripts/lib/selfhost_pkg_workspace_manifest_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_pkg_workspace_manifest_authority_v0.1.json \
  --schema docs/spec/SELFHOST_PKG_WORKSPACE_MANIFEST_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.e package-manifest authority is partial: GenesisCode owns structural
# admission for CLI, obligations, and patches; named effects routes remain H0.
python3 scripts/lib/selfhost_pkg_package_manifest_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_pkg_package_manifest_authority_v0.1.json \
  --schema docs/spec/SELFHOST_PKG_PACKAGE_MANIFEST_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.e workspace-environment selection authority is partial: GenesisCode owns
# backend precedence, normalization, source attribution, and compatibility.
python3 scripts/lib/selfhost_pkg_workspace_env_select_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_pkg_workspace_env_select_authority_v0.1.json \
  --schema docs/spec/SELFHOST_PKG_WORKSPACE_ENV_SELECT_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.e workspace-environment authority is partial: GenesisCode owns exact
# projection, effective inputs, descriptor bodies, identities, and write plan;
# bounded observations, preflight, and atomic persistence remain host mechanisms.
python3 scripts/lib/selfhost_pkg_workspace_env_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_pkg_workspace_env_authority_v0.1.json \
  --schema docs/spec/SELFHOST_PKG_WORKSPACE_ENV_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.e workspace-task authority is partial: GenesisCode owns composed backend
# admission, task lookup, command/argument grammar, and canonical action facts.
python3 scripts/lib/selfhost_pkg_workspace_task_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_pkg_workspace_task_authority_v0.1.json \
  --schema docs/spec/SELFHOST_PKG_WORKSPACE_TASK_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.e semver selection is partial: GenesisCode owns policy extrema and
# lexical tie-breaking; semver parsing, ranking, refs, and transport remain H0.
python3 scripts/lib/selfhost_pkg_semver_select_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_pkg_semver_select_authority_v0.1.json \
  --schema docs/spec/SELFHOST_PKG_SEMVER_SELECT_AUTHORITY_v0.1.schema.json \
  --self-test

# R4.2.e package publication authority is partial: GenesisCode exclusively owns
# policy, evidence, signer, provenance, and sync-plan decisions; transport remains H0.
python3 scripts/lib/selfhost_pkg_publish_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_pkg_publish_authority_v0.1.json \
  --schema docs/spec/SELFHOST_PKG_PUBLISH_AUTHORITY_v0.1.schema.json \
  --artifact selfhost/toolchain.gc \
  --self-test

# R4.2.d obligation authority is H2 for every declared obligation kind; Rust
# retains bounded observations, execution mechanisms, and contradiction checks.
python3 scripts/lib/selfhost_obligation_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_obligation_authority_v0.1.json \
  --schema docs/spec/SELFHOST_OBLIGATION_AUTHORITY_v0.1.schema.json \
  --self-test

python3 scripts/lib/semantic_ownership_ledger.py \
  --root "$ROOT_DIR" \
  --ledger docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json \
  --schema docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.schema.json \
  --spec docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.md \
  --closure docs/spec/SELFHOST_CLOSURE_LEVELS_v0.1.json \
  --self-test

# Status projections must preserve the route/authority/bootstrap distinctions.
bash scripts/check_selfhost_doc_runtime_parity.sh

resolve_base() {
  if [[ -n "${SELFHOST_BOUNDARY_BASE:-}" ]]; then
    echo "$SELFHOST_BOUNDARY_BASE"
    return 0
  fi

  if [[ -n "${GITHUB_BASE_REF:-}" ]]; then
    if declare -F genesis_gate_telemetry_event >/dev/null 2>&1; then
      genesis_gate_telemetry_event network-attempt 1
    fi
    git fetch --no-tags --depth=1 origin "$GITHUB_BASE_REF" >/dev/null 2>&1 || true
    local mb
    mb="$(git merge-base HEAD "origin/${GITHUB_BASE_REF}" 2>/dev/null || true)"
    if [[ -n "$mb" ]]; then
      echo "$mb"
      return 0
    fi
  fi

  if git rev-parse HEAD~1 >/dev/null 2>&1; then
    echo "HEAD~1"
    return 0
  fi

  echo ""
}

diff_adds_semantic_tokens() {
  local before="$1"
  local after="$2"
  if { diff -U0 "$before" "$after" || true; } \
    | grep -E '^\+' \
    | grep -Eq "$SEMANTIC_TOKEN_REGEX"; then
    return 0
  fi
  return 1
}

check_added_semantic_tokens() {
  local base_ref="$1"
  local file="$2"
  local before after found
  before="$(mktemp)"
  after="$(mktemp)"
  git show "${base_ref}:${file}" 2>/dev/null \
    | awk '/^#\[cfg\(test\)\]/{exit} {print}' >"$before" || true
  awk '/^#\[cfg\(test\)\]/{exit} {print}' "$file" >"$after"
  found=1
  if diff_adds_semantic_tokens "$before" "$after"; then
    found=0
  fi
  rm -f "$before" "$after"
  return "$found"
}

check_full_file_semantic_tokens() {
  local file="$1"
  awk '/^#\[cfg\(test\)\]/{exit} {print}' "$file" | grep -Eq "$SEMANTIC_TOKEN_REGEX"
}

scanner_self_test() {
  local directory before production test_only
  directory="$(mktemp -d)"
  before="$directory/before.rs"
  production="$directory/production.rs"
  test_only="$directory/test_only.rs"
  printf '%s\n' 'fn boundary() {}' >"$before"
  printf '%s\n' 'fn boundary() { eval_term(); }' >"$production"
  printf '%s\n' 'fn boundary() {}' '#[cfg(test)]' 'mod tests { fn test() { eval_term(); } }' \
    | awk '/^#\[cfg\(test\)\]/{exit} {print}' >"$test_only"
  if ! diff_adds_semantic_tokens "$before" "$production"; then
    echo "selfhost-boundary scanner self-test failed: production addition was missed" >&2
    rm -rf "$directory"
    exit 1
  fi
  if diff_adds_semantic_tokens "$before" "$test_only"; then
    echo "selfhost-boundary scanner self-test failed: test-only addition was treated as production" >&2
    rm -rf "$directory"
    exit 1
  fi
  rm -rf "$directory"
  echo "selfhost-boundary-scanner-self-test: ok (controls=2)"
}

scanner_self_test

list_production_rust_files() {
  if command -v rg >/dev/null 2>&1; then
    rg --files crates --glob 'crates/*/src/**/*.rs' \
      | grep -v '^crates/gc_runtime_bench/' \
      | sort
    return 0
  fi
  find crates -type f -name '*.rs' \
    | grep '/src/' \
    | grep -v '^crates/gc_runtime_bench/' \
    | sort
}

print_semantic_matches() {
  local file="$1"
  if command -v rg >/dev/null 2>&1; then
    rg -n "$SEMANTIC_TOKEN_REGEX" "$file" | sed -n '1,5p'
    return 0
  fi
  grep -En "$SEMANTIC_TOKEN_REGEX" "$file" | sed -n '1,5p'
}

FILES_TO_SCAN=""
BASE_REF=""
if [[ "$MODE" == "strict" ]]; then
  FILES_TO_SCAN="$(list_production_rust_files)"
  if [[ -z "$FILES_TO_SCAN" ]]; then
    echo "selfhost-boundary: no Rust files under crates/."
    exit 0
  fi
else
  BASE_REF="$(resolve_base)"
  if [[ -z "$BASE_REF" ]]; then
    echo "selfhost-boundary: no diff base detected; escalating to strict mode."
    MODE="strict"
    FILES_TO_SCAN="$(list_production_rust_files)"
    if [[ -z "$FILES_TO_SCAN" ]]; then
      echo "selfhost-boundary: no Rust files under crates/."
      exit 0
    fi
  else
    FILES_TO_SCAN="$({
      git diff --name-only "$BASE_REF" -- 'crates/**/*.rs'
      git ls-files --others --exclude-standard -- 'crates/**/*.rs'
    } | sort -u)"
    if [[ -z "$FILES_TO_SCAN" ]]; then
      echo "selfhost-boundary: no changed Rust files under crates/."
      exit 0
    fi
  fi
fi

violations=0

while IFS= read -r file; do
  [[ -n "$file" ]] || continue
  [[ -f "$file" ]] || continue

  if is_allowed_path "$file"; then
    continue
  fi

  if [[ "$MODE" == "strict" ]]; then
    if check_full_file_semantic_tokens "$file"; then
      echo "selfhost-boundary violation (strict): semantic token in non-approved file: $file"
      print_semantic_matches "$file"
      violations=$((violations + 1))
    fi
  else
    if check_added_semantic_tokens "$BASE_REF" "$file"; then
      echo "selfhost-boundary violation (diff): semantic token added in non-approved file: $file"
      violations=$((violations + 1))
    fi
  fi
done <<EOF
$FILES_TO_SCAN
EOF

if [[ "$violations" -gt 0 ]]; then
  cat <<'EOF'
selfhost-boundary: failed.
Do not add new Rust language-semantic surface outside approved modules.
Move semantic logic into .gc toolchain modules and keep Rust as host/runtime boundary.
EOF
  exit 1
fi

echo "selfhost-boundary: ok (mode=$MODE)"
