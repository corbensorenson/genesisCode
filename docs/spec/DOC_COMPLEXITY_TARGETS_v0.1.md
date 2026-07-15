# Documentation Complexity Targets v0.1

Last updated: 2026-07-15

Purpose: set fail-closed measurable complexity budgets for AI retrieval quality.

## Metrics

- `active_docs_md`: markdown files under `docs/` excluding redirect stubs that
  carry the top-level deprecation marker.
- `active_top_level_leaf_docs`: non-deprecated `docs/*.md` leaf docs excluding
  infrastructure docs (`docs/INDEX.md`, `docs/DEPRECATION_MAP_v0.1.md`).
- `capability_retrieval_fanout`: `primary_evidence_paths / capability_rows`
  from `feature_matrix.md`.

## Targets (v0.1)

- `active_docs_md <= 135`
- `active_top_level_leaf_docs <= 6`
- `capability_retrieval_fanout <= 0.46`

## Enforcement

- Policy file: `policies/docs/doc_complexity_budget.toml`
- Gate: `scripts/check_doc_complexity_budget.sh`
- Read-only check: `bash scripts/check_doc_complexity_budget.sh` renders into a temporary directory.
- Explicit local report update: `bash scripts/update_doc_complexity_report.sh` writes `.genesis/perf/doc_complexity_report.json`.

These targets are designed to reduce retrieval ambiguity for agent authoring
while preserving required normative coverage.
