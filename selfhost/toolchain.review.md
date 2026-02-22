# Selfhost Toolchain Review Sidecar (v0.1)

Deterministic review-sidecar for `selfhost/toolchain.gc`.

## Artifact Identity

- Artifact path: `selfhost/toolchain.gc`
- Artifact sha256: `3b8c00eae949b689b4b442709f6f38e083f07e3c7ad2ac1e5e43f43bc7756c8e`
- Freshness artifact hash: `3b8c00eae949b689b4b442709f6f38e083f07e3c7ad2ac1e5e43f43bc7756c8e`
- Freshness source hash: `dfd5048e0316b049868b0a33961434c59c1d6f4c6d4aa918ad824d13daa0caa3`
- Source aggregate hash (module path + module sha256): `88cf702e16d54c8d1d43265d1b8e4c9a92353be6cdbf361b17b84485b12ddb18`
- Manifest path: `selfhost/toolchain_manifest.gc`
- Module count: `16`

## Module Summary

| Module | Lines | Bytes | Defs | SHA256 |
| --- | ---: | ---: | ---: | --- |
| `selfhost/parse.gc` | 574 | 26180 | 41 | `4fc71c783f5a9213` |
| `selfhost/canon.gc` | 496 | 20780 | 59 | `b15225e443e83bbb` |
| `selfhost/printer.gc` | 510 | 21131 | 44 | `425954a350839a13` |
| `selfhost/hash.gc` | 28 | 1055 | 5 | `54f123181935d454` |
| `selfhost/tool_coreform_v1.gc` | 28 | 1101 | 4 | `cc8247e0a315fff3` |
| `selfhost/cli_coreform_v1.gc` | 429 | 18040 | 42 | `f405f69443475c39` |
| `selfhost/cli_coreform_vcs_queries_v1.gc` | 539 | 26219 | 26 | `35126ef4174536a9` |
| `selfhost/cli_coreform_vcs_pkg_v1.gc` | 496 | 24437 | 21 | `94d44289883538c1` |
| `selfhost/cli_pkg_runtime_v1.gc` | 541 | 30243 | 16 | `00657df418c94a09` |
| `selfhost/cli_pkg_runtime_verify_v1.gc` | 237 | 11535 | 14 | `4673d497fcb0e2ad` |
| `selfhost/cli_pkg_ops_v1.gc` | 421 | 21222 | 21 | `7bd3d718d554a167` |
| `selfhost/cli_reachability_v1.gc` | 613 | 23537 | 41 | `07b2f4605ea01c9d` |
| `selfhost/cli_reachability_closure_v1.gc` | 297 | 13959 | 21 | `0c82e8d660c258b3` |
| `selfhost/patch_schema_v1.gc` | 525 | 26496 | 42 | `28584a60caef1c2b` |
| `selfhost/patch_schema_manifest_v1.gc` | 316 | 14008 | 28 | `9ab3286173641429` |
| `selfhost/stage1_v1.gc` | 398 | 16588 | 47 | `332a04e971ddc71b` |

## Export Surface (Preview)

- `selfhost/parse.gc`: `selfhost/parse::error`, `selfhost/parse::is-error`, `selfhost/parse::SYM_QUOTE`, `selfhost/parse::byte`, `selfhost/parse::is-ws?`, `selfhost/parse::is-delim?`, `selfhost/parse::skip-ws-and-comments`, `selfhost/parse::skip-comment`
- `selfhost/canon.gc`: `selfhost/canon::is-error`, `selfhost/canon::bad-form`, `selfhost/canon::type-error`, `selfhost/canon::tag`, `selfhost/canon::SYM_QUOTE`, `selfhost/canon::SYM_DEF`, `selfhost/canon::SYM_FN`, `selfhost/canon::SYM_IF`
- `selfhost/printer.gc`: `selfhost/printer::is-error`, `selfhost/printer::tag`, `selfhost/printer::INDENT`, `selfhost/printer::MAX_WIDTH`, `selfhost/printer::spaces`, `selfhost/printer::list-rev`, `selfhost/printer::list-rev2`, `selfhost/printer::append-lines-to-rev`
- `selfhost/hash.gc`: `selfhost/hash::is-error`, `selfhost/hash::PREFIX_TERM`, `selfhost/hash::PREFIX_MODULE`, `selfhost/hash::hash-term`, `selfhost/hash::hash-module`
- `selfhost/tool_coreform_v1.gc`: `selfhost/tool::is-error`, `selfhost/tool::fmt-module`, `selfhost/tool::hash-module-src`, `selfhost/tool::hash-src-with-kind`
- `selfhost/cli_coreform_v1.gc`: `core/cli::is-error`, `core/cli::sym?`, `core/cli::vec?`, `core/cli::map?`, `core/cli::pair?`, `core/cli::str?`, `core/cli::bool?`, `core/cli::int?`
- `selfhost/cli_coreform_vcs_queries_v1.gc`: `core/cli::vcs-log::vec-take`, `core/cli::vcs-log::vec-take2`, `core/cli::vcs-log::vec-reverse`, `core/cli::vcs-log::vec-reverse2`, `core/cli::vcs-log::entry`, `core/cli::vcs-log::resolve-root`, `core/cli::vcs-log::loop`, `core/cli::vcs-log-program`
- `selfhost/cli_coreform_vcs_pkg_v1.gc`: `core/cli::store-put-hash`, `core/cli::store-get-artifact`, `core/cli::vcs-read-term-file-program`, `core/cli::vcs-write-term-file-program`, `core/cli::vcs-write-out-if-needed-program`, `core/cli::vcs-resolve-patch-term-program`, `core/cli::vcs-load-hash-term-program`, `core/cli::vcs-diff-low-program`
- `selfhost/cli_pkg_runtime_v1.gc`: `core/cli::pkg-lock-program`, `core/cli::vcs-validate-attestation`, `core/cli::pkg-ensure-hash-program`, `core/cli::pkg-validate-evidence-loop-program`, `core/cli::pkg-validate-attestations-loop-program`, `core/cli::pkg-validate-commit-closure-program`, `core/cli::pkg-lock-strict-selector-check`, `core/cli::pkg-lock-strict-validate-entry-program`
- `selfhost/cli_pkg_runtime_verify_v1.gc`: `core/cli::pkg-has-present`, `core/cli::pkg-checked-count`, `core/cli::pkg-missing-vec`, `core/cli::pkg-state-add-checked`, `core/cli::pkg-state-add-missing`, `core/cli::pkg-check-hashes-loop-program`, `core/cli::pkg-requirements-missing-locks`, `core/cli::pkg-requirements-missing-locks2`
- `selfhost/cli_pkg_ops_v1.gc`: `core/cli::pkg-load-lock-program`, `core/cli::pkg-list-requirements`, `core/cli::pkg-list-requirements2`, `core/cli::pkg-list-locked`, `core/cli::pkg-list-locked2`, `core/cli::pkg-info-requirement-view`, `core/cli::pkg-info-locked-view`, `core/cli::pkg-list-program`
- `selfhost/cli_reachability_v1.gc`: `core/cli::empty-vec`, `core/cli::vec2`, `core/cli::vec-slice-from`, `core/cli::vec-slice-from2`, `core/cli::literal-op-sym-or-nil`, `core/cli::flatten-app`, `core/cli::infer-effects`, `core/cli::infer-effects-vec`
- `selfhost/cli_reachability_closure_v1.gc`: `core/cli::vec-append`, `core/cli::vec-append2`, `core/cli::push-if-hash`, `core/cli::push-hashes-from-vec`, `core/cli::push-hashes-from-vec2`, `core/cli::map-entries-vec`, `core/cli::push-hash-values-from-map`, `core/cli::push-hash-values-from-map2`
- `selfhost/patch_schema_v1.gc`: `selfhost/patch_schema::is-int?`, `selfhost/patch_schema::is-str?`, `selfhost/patch_schema::is-sym?`, `selfhost/patch_schema::is-sym-or-str?`, `selfhost/patch_schema::is-vec?`, `selfhost/patch_schema::is-map?`, `selfhost/patch_schema::err`, `selfhost/patch_schema::require`
- `selfhost/patch_schema_manifest_v1.gc`: `selfhost/patch_schema::empty-vec`, `selfhost/patch_schema_manifest::err`, `selfhost/patch_schema::str?`, `selfhost/patch_schema::sym?`, `selfhost/patch_schema::vec?`, `selfhost/patch_schema::map?`, `selfhost/patch_schema::key->str`, `selfhost/patch_schema::require-map-field`
- `selfhost/stage1_v1.gc`: `selfhost/stage1::is-error`, `selfhost/stage1::type-error`, `selfhost/stage1::bad-form`, `selfhost/stage1::tag`, `selfhost/stage1::empty-vec`, `selfhost/stage1::vec1`, `selfhost/stage1::vec2`, `selfhost/stage1::vec3`
