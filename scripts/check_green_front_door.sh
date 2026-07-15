#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "green-front-door: docs quickstart"
bash scripts/check_docs_quickstart.sh

echo "green-front-door: docs freshness/topology/hygiene/complexity"
bash scripts/check_planning_docs_fresh.sh
bash scripts/check_roadmap_execution_manifest.sh
bash scripts/check_doc_topology_drift.sh
bash scripts/check_doc_hygiene.sh
bash scripts/check_doc_complexity_budget.sh
bash scripts/check_root_lock_policy.sh
bash scripts/check_prerequisite_manifest.sh
bash scripts/check_reference_host_profiles.sh
bash scripts/check_roadmap_workloads.sh
bash scripts/check_roadmap_baseline.sh
bash scripts/check_dependency_mirror_contract.sh

echo "green-front-door: generated artifacts, versioning, supply chain, release smoke"
bash scripts/check_capability_indices.sh
bash scripts/check_selfhost_dashboard_fresh.sh
bash scripts/check_check_update_boundary.sh
bash scripts/check_deterministic_cleanup.sh
bash scripts/check_gate_resource_telemetry.sh
bash scripts/check_gate_manifest.sh
bash scripts/check_engineering_gate_contract.sh
bash scripts/check_genesis_evidence_profile.sh
bash scripts/check_genesis_evidence_verifier.sh
bash scripts/check_evidence_adversarial_matrix.sh
bash scripts/check_evidence_storage_classes.sh
bash scripts/check_generated_artifact_policy.sh
bash scripts/check_gc_agent_core_card.sh
bash scripts/check_gc_agent_profile.sh
bash scripts/check_gc_agent_task_cards.sh
bash scripts/check_gc_agent_symbol_index.sh
bash scripts/check_release_notes.sh
bash scripts/check_version_surfaces.sh
bash scripts/check_v1_compatibility.sh
bash scripts/check_versioning_release_hygiene.sh
bash scripts/check_supply_chain.sh
bash scripts/check_release_smoke.sh

echo "green-front-door: invariant guards"
bash scripts/check_no_user_panics.sh
bash scripts/check_kernel_tcb_contract.sh
bash scripts/check_selfhost_boundary.sh --strict

echo "green-front-door: test profile matrix"
bash scripts/check_changed_impact.sh
bash scripts/check_test_execution_profile_matrix.sh

echo "green-front-door: changed-fast loop"
bash scripts/test_changed_fast.sh --runner auto --min-history 1

echo "green-front-door: ok"
