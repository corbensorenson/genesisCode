# Selfhost Toolchain Review Sidecar (v0.1)

Deterministic review-sidecar for `selfhost/toolchain.gc`.

## Artifact Identity

- Artifact path: `selfhost/toolchain.gc`
- Artifact sha256: `716c8c8c4b8547cb5b39f1248db8b05f43966d3d9564c648b42da34e842f2550`
- Freshness artifact hash: `716c8c8c4b8547cb5b39f1248db8b05f43966d3d9564c648b42da34e842f2550`
- Freshness source hash: `d9c34d162f0856cbf68f94b039e85f18e20c3e6f716d7c6e64accbf6030f75c6`
- Source aggregate hash (module path + module sha256): `baac85d67cae860be87e784d3cacc89f7fad528f5f8f5a69f6a4c4767a2843c5`
- Manifest path: `selfhost/toolchain_manifest.gc`
- Module count: `25`

## Module Summary

| Module | Lines | Bytes | Defs | SHA256 |
| --- | ---: | ---: | ---: | --- |
| `selfhost/parse.gc` | 389 | 17882 | 32 | `c703e7343f2d871a` |
| `selfhost/parse_core_v1.gc` | 184 | 8297 | 9 | `043012c45aa0e0f3` |
| `selfhost/canon.gc` | 496 | 20780 | 59 | `b15225e443e83bbb` |
| `selfhost/printer/00_core_single_line.gc` | 208 | 7799 | 23 | `b3830b4d2e39fb31` |
| `selfhost/printer/01_single_line_list.gc` | 38 | 1452 | 3 | `5f1d744a14a38286` |
| `selfhost/printer/02_fmt_structured.gc` | 131 | 6260 | 8 | `bbe116ae8dc10b8b` |
| `selfhost/printer/03_fmt_list_module.gc` | 133 | 5620 | 10 | `de51e99e34b710f5` |
| `selfhost/hash.gc` | 28 | 1055 | 5 | `54f123181935d454` |
| `selfhost/tool_coreform_v1.gc` | 28 | 1101 | 4 | `cc8247e0a315fff3` |
| `selfhost/cli_coreform_v1.gc` | 429 | 18040 | 42 | `f405f69443475c39` |
| `selfhost/cli_coreform_vcs_queries_v1.gc` | 210 | 9125 | 13 | `96fbbb8f4b9f1e06` |
| `selfhost/cli_coreform_vcs_blame_v1.gc` | 327 | 17092 | 13 | `24b55511a286997b` |
| `selfhost/cli_coreform_vcs_pkg_v1.gc` | 465 | 22278 | 21 | `befd359a9422d577` |
| `selfhost/cli_pkg_runtime_v1.gc` | 233 | 13168 | 8 | `7f05a6398597ecdd` |
| `selfhost/cli_pkg_runtime_updates_v1.gc` | 201 | 10219 | 8 | `d872ba3597eda520` |
| `selfhost/cli_pkg_runtime_verify_v1.gc` | 243 | 11706 | 14 | `5267629fe45699a5` |
| `selfhost/cli_pkg_ops_v1.gc` | 421 | 21222 | 21 | `7bd3d718d554a167` |
| `selfhost/cli_reachability_v1.gc` | 398 | 14168 | 24 | `2dae442240dbd8b8` |
| `selfhost/cli_reachability_rules_v1.gc` | 218 | 9499 | 17 | `bfc421a3bc682517` |
| `selfhost/cli_reachability_closure_v1.gc` | 297 | 13959 | 21 | `0c82e8d660c258b3` |
| `selfhost/patch_schema_v1.gc` | 356 | 17578 | 32 | `2d406bff742c79a1` |
| `selfhost/patch_schema_apply_v1.gc` | 163 | 8681 | 10 | `59117ea31903a5b8` |
| `selfhost/patch_schema_manifest_v1.gc` | 406 | 17939 | 33 | `a1f23ac0a6708185` |
| `selfhost/patch_schema_refactor_v1.gc` | 935 | 45840 | 61 | `6824fd2deced077f` |
| `selfhost/stage1_v1.gc` | 398 | 16588 | 47 | `332a04e971ddc71b` |

## Export Surface (Preview)

- `selfhost/parse.gc`: `selfhost/parse::error`, `selfhost/parse::is-error`, `selfhost/parse::SYM_QUOTE`, `selfhost/parse::byte`, `selfhost/parse::is-ws?`, `selfhost/parse::is-delim?`, `selfhost/parse::skip-ws-and-comments`, `selfhost/parse::skip-comment`
- `selfhost/parse_core_v1.gc`: `selfhost/parse::parse-symbol-or-int`, `selfhost/parse::parse-symbol`, `selfhost/parse::parse-list`, `selfhost/parse::parse-list2`, `selfhost/parse::parse-vector`, `selfhost/parse::parse-map`, `selfhost/parse::parse-term`, `selfhost/parse::parse-module`
- `selfhost/canon.gc`: `selfhost/canon::is-error`, `selfhost/canon::bad-form`, `selfhost/canon::type-error`, `selfhost/canon::tag`, `selfhost/canon::SYM_QUOTE`, `selfhost/canon::SYM_DEF`, `selfhost/canon::SYM_FN`, `selfhost/canon::SYM_IF`
- `selfhost/printer/00_core_single_line.gc`: `selfhost/printer::is-error`, `selfhost/printer::tag`, `selfhost/printer::INDENT`, `selfhost/printer::MAX_WIDTH`, `selfhost/printer::spaces`, `selfhost/printer::list-rev`, `selfhost/printer::list-rev2`, `selfhost/printer::append-lines-to-rev`
- `selfhost/printer/01_single_line_list.gc`: `selfhost/printer::single-line-list`, `selfhost/printer::vec-single-lines`, `selfhost/printer::vec-single-lines2`
- `selfhost/printer/02_fmt_structured.gc`: `selfhost/printer::fmt-term`, `selfhost/printer::fmt-term2`, `selfhost/printer::fmt-vector`, `selfhost/printer::fmt-vector2`, `selfhost/printer::fmt-map`, `selfhost/printer::fmt-map2`, `selfhost/printer::fmt-map-entry`, `selfhost/printer::fmt-map-entry-multiline`
- `selfhost/printer/03_fmt_list_module.gc`: `selfhost/printer::fmt-list`, `selfhost/printer::fmt-list-items`, `selfhost/printer::fmt-list-headless`, `selfhost/printer::fmt-list-headed`, `selfhost/printer::list-head-count`, `selfhost/printer::list-first-line`, `selfhost/printer::fmt-list-tail`, `selfhost/printer::print-term`
- `selfhost/hash.gc`: `selfhost/hash::is-error`, `selfhost/hash::PREFIX_TERM`, `selfhost/hash::PREFIX_MODULE`, `selfhost/hash::hash-term`, `selfhost/hash::hash-module`
- `selfhost/tool_coreform_v1.gc`: `selfhost/tool::is-error`, `selfhost/tool::fmt-module`, `selfhost/tool::hash-module-src`, `selfhost/tool::hash-src-with-kind`
- `selfhost/cli_coreform_v1.gc`: `core/cli::is-error`, `core/cli::sym?`, `core/cli::vec?`, `core/cli::map?`, `core/cli::pair?`, `core/cli::str?`, `core/cli::bool?`, `core/cli::int?`
- `selfhost/cli_coreform_vcs_queries_v1.gc`: `core/cli::vcs-log::vec-take`, `core/cli::vcs-log::vec-take2`, `core/cli::vcs-log::vec-reverse`, `core/cli::vcs-log::vec-reverse2`, `core/cli::vcs-log::entry`, `core/cli::vcs-log::resolve-root`, `core/cli::vcs-log::loop`, `core/cli::vcs-log-program`
- `selfhost/cli_coreform_vcs_blame_v1.gc`: `core/cli::vcs-load-commit`, `core/cli::vcs-load-snapshot`, `core/cli::vcs-ref-hashes`, `core/cli::vcs-ref-hashes2`, `core/cli::vcs-find-commit-for-snapshot-loop`, `core/cli::vcs-find-commit-for-snapshot`, `core/cli::vcs-snapshot-symbol-ref-by-hash`, `core/cli::vcs-blame-next-parent-loop`
- `selfhost/cli_coreform_vcs_pkg_v1.gc`: `core/cli::store-put-hash`, `core/cli::store-get-artifact`, `core/cli::vcs-read-term-file-program`, `core/cli::vcs-write-term-file-program`, `core/cli::vcs-write-out-if-needed-program`, `core/cli::vcs-resolve-patch-term-program`, `core/cli::vcs-load-hash-term-program`, `core/cli::vcs-diff-low-program`
- `selfhost/cli_pkg_runtime_v1.gc`: `core/cli::pkg-lock-program`, `core/cli::vcs-validate-attestation`, `core/cli::pkg-ensure-hash-program`, `core/cli::pkg-validate-evidence-loop-program`, `core/cli::pkg-validate-attestations-loop-program`, `core/cli::pkg-validate-commit-closure-program`, `core/cli::pkg-lock-strict-selector-check`, `core/cli::pkg-lock-strict-validate-entry-program`
- `selfhost/cli_pkg_runtime_updates_v1.gc`: `core/cli::pkg-lock-loop`, `core/cli::str-drop-prefix`, `core/cli::pkg-selector-value`, `core/cli::pkg-update-policy-auto?`, `core/cli::pkg-resolve-from-commit-program`, `core/cli::pkg-resolve-requirement-program`, `core/cli::pkg-update-loop`, `core/cli::pkg-update-program`
- `selfhost/cli_pkg_runtime_verify_v1.gc`: `core/cli::pkg-has-present`, `core/cli::pkg-checked-count`, `core/cli::pkg-missing-vec`, `core/cli::pkg-state-add-checked`, `core/cli::pkg-state-add-missing`, `core/cli::pkg-check-hashes-loop-program`, `core/cli::pkg-requirements-missing-locks`, `core/cli::pkg-requirements-missing-locks2`
- `selfhost/cli_pkg_ops_v1.gc`: `core/cli::pkg-load-lock-program`, `core/cli::pkg-list-requirements`, `core/cli::pkg-list-requirements2`, `core/cli::pkg-list-locked`, `core/cli::pkg-list-locked2`, `core/cli::pkg-info-requirement-view`, `core/cli::pkg-info-locked-view`, `core/cli::pkg-list-program`
- `selfhost/cli_reachability_v1.gc`: `core/cli::empty-vec`, `core/cli::vec2`, `core/cli::vec-slice-from`, `core/cli::vec-slice-from2`, `core/cli::literal-op-sym-or-nil`, `core/cli::flatten-app`, `core/cli::infer-effects`, `core/cli::infer-effects-vec`
- `selfhost/cli_reachability_rules_v1.gc`: `core/cli::set-keys->vec`, `core/cli::set-keys->vec2`, `core/cli::reach-str?`, `core/cli::reach-int?`, `core/cli::hash-hex?`, `core/cli::hash-vec?`, `core/cli::hash-vec2`, `core/cli::vcs-make-commit`
- `selfhost/cli_reachability_closure_v1.gc`: `core/cli::vec-append`, `core/cli::vec-append2`, `core/cli::push-if-hash`, `core/cli::push-hashes-from-vec`, `core/cli::push-hashes-from-vec2`, `core/cli::map-entries-vec`, `core/cli::push-hash-values-from-map`, `core/cli::push-hash-values-from-map2`
- `selfhost/patch_schema_v1.gc`: `selfhost/patch_schema::is-int?`, `selfhost/patch_schema::is-str?`, `selfhost/patch_schema::is-sym?`, `selfhost/patch_schema::is-sym-or-str?`, `selfhost/patch_schema::is-vec?`, `selfhost/patch_schema::is-map?`, `selfhost/patch_schema::err`, `selfhost/patch_schema::require`
- `selfhost/patch_schema_apply_v1.gc`: `selfhost/patch_schema::vec-replace-loop`, `selfhost/patch_schema::step-tag`, `selfhost/patch_schema::apply-replace-step-sym`, `selfhost/patch_schema::apply-replace-step`, `selfhost/patch_schema::apply-replace-term`, `core/cli::apply-replace-node`, `core/cli::print-module-forms`, `core/cli::canonicalize-module-content`
- `selfhost/patch_schema_manifest_v1.gc`: `selfhost/patch_schema::empty-vec`, `selfhost/patch_schema_manifest::err`, `selfhost/patch_schema::str?`, `selfhost/patch_schema::sym?`, `selfhost/patch_schema::vec?`, `selfhost/patch_schema::map?`, `selfhost/patch_schema::key->str`, `selfhost/patch_schema::require-map-field`
- `selfhost/patch_schema_refactor_v1.gc`: `selfhost/patch_refactor::is-error`, `selfhost/patch_refactor::err`, `selfhost/patch_refactor::empty-vec`, `selfhost/patch_refactor::vec2`, `selfhost/patch_refactor::res`, `selfhost/patch_refactor::res-term`, `selfhost/patch_refactor::res-count`, `selfhost/patch_refactor::rename-result`
- `selfhost/stage1_v1.gc`: `selfhost/stage1::is-error`, `selfhost/stage1::type-error`, `selfhost/stage1::bad-form`, `selfhost/stage1::tag`, `selfhost/stage1::empty-vec`, `selfhost/stage1::vec1`, `selfhost/stage1::vec2`, `selfhost/stage1::vec3`
