# Selfhost Toolchain Review Sidecar (v0.1)

Deterministic review-sidecar for `selfhost/toolchain.gc`.

## Artifact Identity

- Artifact path: `selfhost/toolchain.gc`
- Artifact sha256: `b0ce6ee3f176f5790ee443d9b14747ffc68bc4765c7e7e768dd4a381523be2fa`
- Freshness artifact hash: `b0ce6ee3f176f5790ee443d9b14747ffc68bc4765c7e7e768dd4a381523be2fa`
- Freshness source hash: `b495b61cb1ee4d04b2df0c60790987b9c2aa32a9b6efec34b734243b923902ba`
- Source aggregate hash (module path + module sha256): `b9b88b7720cbcb4c63202dc4cd1dec648e11c1b4f31f8e3690c2ddb0b7589753`
- Manifest path: `selfhost/toolchain_manifest.gc`
- Module count: `76`

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
| `selfhost/cli_coreform_v1.gc` | 453 | 19245 | 43 | `8c7e539dd64a35b5` |
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
| `selfhost/typecheck_core_v1.gc` | 241 | 8119 | 43 | `5b2830c81fd26f4b` |
| `selfhost/typecheck_types_v1.gc` | 403 | 19013 | 39 | `b7a8e2d27a56f1c6` |
| `selfhost/typecheck_compat_v1.gc` | 650 | 29109 | 33 | `3e1f2ce06ff0e3c9` |
| `selfhost/typecheck_infer_apply_v1.gc` | 168 | 8162 | 6 | `aa7cab6697449377` |
| `selfhost/typecheck_infer_prim_v1.gc` | 266 | 13774 | 16 | `a5ce190b718fd6e9` |
| `selfhost/typecheck_infer_core_v1.gc` | 549 | 25892 | 30 | `33b7afd6f8f3e6f3` |
| `selfhost/typecheck_infer_contract_v1.gc` | 247 | 13495 | 7 | `d2983313e1643216` |
| `selfhost/typecheck_infer_effect_v1.gc` | 246 | 12871 | 11 | `91b0db8eb51fc8f3` |
| `selfhost/typecheck_infer_app_v1.gc` | 52 | 2975 | 2 | `37717b57a1889f67` |
| `selfhost/typecheck_typed_effects_v1.gc` | 459 | 19025 | 29 | `b107915572a5a97c` |
| `selfhost/typecheck_unknown_signatures_v1.gc` | 284 | 10852 | 15 | `a4a29935a360a898` |
| `selfhost/typecheck_package_meta_v1.gc` | 179 | 7022 | 19 | `9538a470ef806390` |
| `selfhost/typecheck_module_profile_descriptor_v1.gc` | 537 | 22236 | 32 | `92079ecb7f6059cc` |
| `selfhost/typecheck_module_profile_references_v1.gc` | 280 | 10816 | 12 | `b9b912429ee5502e` |
| `selfhost/typecheck_module_profile_resolution_v1.gc` | 671 | 26954 | 39 | `fa393406afad8487` |
| `selfhost/typecheck_profile_negotiation_v1.gc` | 565 | 24821 | 32 | `a0175e2cdb0a40d2` |
| `selfhost/typecheck_contract_profile_v1.gc` | 552 | 23757 | 26 | `cbe864f1462a92f2` |
| `selfhost/typecheck_contract_profile_compose_v1.gc` | 306 | 13030 | 16 | `aff6753bdde8a76b` |
| `selfhost/typecheck_package_context_v1.gc` | 205 | 8451 | 12 | `4b03250139f9bc8b` |
| `selfhost/typecheck_package_module_v1.gc` | 208 | 8545 | 15 | `c60cd21982394e96` |
| `selfhost/typecheck_package_exports_v1.gc` | 441 | 18981 | 18 | `b792e2c0340a0127` |
| `selfhost/typecheck_package_report_v1.gc` | 177 | 7165 | 11 | `e38ce8e915a1fc7a` |
| `selfhost/patch_schema_v1.gc` | 356 | 17578 | 32 | `2d406bff742c79a1` |
| `selfhost/patch_schema_apply_v1.gc` | 163 | 8681 | 10 | `59117ea31903a5b8` |
| `selfhost/patch_schema_manifest_v1.gc` | 406 | 17939 | 33 | `a1f23ac0a6708185` |
| `selfhost/patch_schema_refactor_v1.gc` | 523 | 23095 | 46 | `a8be5b06db03d9e2` |
| `selfhost/patch_schema_refactor_meta_migrate_v1.gc` | 411 | 22744 | 15 | `9aa240b249e5a43d` |
| `selfhost/patch_authority_identity_v1.gc` | 223 | 9534 | 16 | `11e3c072866e00cc` |
| `selfhost/patch_authority_normalize_v1.gc` | 522 | 24197 | 30 | `e5759068bf904679` |
| `selfhost/patch_authority_preflight_v1.gc` | 353 | 14390 | 23 | `8acdfdfc451f3766` |
| `selfhost/patch_authority_refactor_plan_v1.gc` | 508 | 23030 | 24 | `bcb27dc8aa6a9c37` |
| `selfhost/patch_authority_diff_v1.gc` | 247 | 10442 | 23 | `b6614dbc996d6e4d` |
| `selfhost/patch_authority_merge_v1.gc` | 275 | 11984 | 21 | `a73aa5b6e94e9048` |
| `selfhost/patch_authority_apply_report_v1.gc` | 136 | 6196 | 12 | `12f15a3a4ab781ff` |
| `selfhost/policy_authority_v1.gc` | 345 | 15515 | 30 | `e7f1616be08a7385` |
| `selfhost/effect_policy_crypto_v1.gc` | 263 | 9717 | 20 | `8c6321d883078c80` |
| `selfhost/effect_policy_network_v1.gc` | 267 | 10410 | 21 | `778de86429de628f` |
| `selfhost/effect_policy_plugin_v1.gc` | 122 | 4427 | 12 | `312264630f9f4219` |
| `selfhost/effect_policy_ffi_v1.gc` | 213 | 9887 | 9 | `76475d2cfeaf1771` |
| `selfhost/effect_policy_authority_v1.gc` | 659 | 28989 | 38 | `04e6ca7d360ce7e6` |
| `selfhost/obligation_authority_core_v1.gc` | 570 | 24016 | 50 | `859184a3ee1f4dba` |
| `selfhost/obligation_authority_typecheck_v1.gc` | 63 | 2820 | 3 | `f73f4ec01ba35a78` |
| `selfhost/obligation_authority_determinism_v1.gc` | 218 | 9254 | 13 | `96bfc11b26ae5a5b` |
| `selfhost/obligation_authority_lint_v1.gc` | 340 | 14378 | 18 | `141e6f21525f3e85` |
| `selfhost/obligation_authority_ai_style_v1.gc` | 220 | 10449 | 9 | `2f1769614f19e319` |
| `selfhost/obligation_authority_preflight_v1.gc` | 113 | 5223 | 7 | `20acd454050b6785` |
| `selfhost/obligation_authority_replay_v1.gc` | 354 | 16695 | 17 | `d8a929727e4a40ea` |
| `selfhost/obligation_authority_property_v1.gc` | 606 | 32356 | 30 | `05c601549992b546` |
| `selfhost/obligation_authority_stage_v1.gc` | 161 | 7200 | 9 | `ca1ff0651febd10d` |
| `selfhost/obligation_authority_coverage_v1.gc` | 523 | 24182 | 29 | `77e5304e98081ef0` |
| `selfhost/obligation_authority_translation_v1.gc` | 547 | 25227 | 24 | `d2f924dbde90c365` |
| `selfhost/obligation_authority_gfx_api_v1.gc` | 247 | 11297 | 16 | `d7824d7cb9d3a46f` |
| `selfhost/obligation_authority_gfx_runtime_v1.gc` | 380 | 20945 | 14 | `94b78ace06c7e7e4` |
| `selfhost/obligation_authority_gfx_runtime_finalize_v1.gc` | 350 | 20934 | 14 | `1e26126bb78036ff` |
| `selfhost/obligation_authority_v1.gc` | 94 | 6163 | 3 | `13bb31f4e2cd316c` |
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
- `selfhost/typecheck_core_v1.gc`: `selfhost/typecheck::empty-vec`, `selfhost/typecheck::probe-ok?`, `selfhost/typecheck::sym?`, `selfhost/typecheck::str?`, `selfhost/typecheck::vec?`, `selfhost/typecheck::map?`, `selfhost/typecheck::pair?`, `selfhost/typecheck::bool?`
- `selfhost/typecheck_types_v1.gc`: `selfhost/typecheck::ty-scalar`, `selfhost/typecheck::ty-any`, `selfhost/typecheck::ty-int`, `selfhost/typecheck::ty-dec`, `selfhost/typecheck::ty-bool`, `selfhost/typecheck::ty-nil`, `selfhost/typecheck::ty-str`, `selfhost/typecheck::ty-bytes`
- `selfhost/typecheck_compat_v1.gc`: `selfhost/typecheck::tail-closed?`, `selfhost/typecheck::tail-any?`, `selfhost/typecheck::tail-var?`, `selfhost/typecheck::effect-scan-empty`, `selfhost/typecheck::collect-effect-tail`, `selfhost/typecheck::collect-effect-rows`, `selfhost/typecheck::collect-effect-fields`, `selfhost/typecheck::collect-effect-fields-loop`
- `selfhost/typecheck_infer_apply_v1.gc`: `selfhost/typecheck::arg-type-match`, `selfhost/typecheck::arg-shape-match`, `selfhost/typecheck::arg-shape-match-loop`, `selfhost/typecheck::arg-type-compatible?`, `selfhost/typecheck::infer-apply-types`, `selfhost/typecheck::infer-apply-types-loop`
- `selfhost/typecheck_infer_prim_v1.gc`: `selfhost/typecheck::prim-int-arith`, `selfhost/typecheck::prim-int-compare`, `selfhost/typecheck::prim-dec-arith`, `selfhost/typecheck::prim-dec-compare`, `selfhost/typecheck::prim-str-lengths`, `selfhost/typecheck::prim-result`, `selfhost/typecheck::prim-fail`, `selfhost/typecheck::prim-arity-message`
- `selfhost/typecheck_infer_core_v1.gc`: `selfhost/typecheck::infer-result`, `selfhost/typecheck::infer-error`, `selfhost/typecheck::env-with-prelude`, `selfhost/typecheck::effects-vec-to-set`, `selfhost/typecheck::effects-vec-to-set-loop`, `selfhost/typecheck::infer-syntax-effects-term`, `selfhost/typecheck::infer-module-types`, `selfhost/typecheck::infer-module-types-loop`
- `selfhost/typecheck_infer_contract_v1.gc`: `selfhost/typecheck::infer-msg-make`, `selfhost/typecheck::infer-msg-payload`, `selfhost/typecheck::infer-contract-extend`, `selfhost/typecheck::infer-contract-overrides`, `selfhost/typecheck::infer-contract-overrides-loop`, `selfhost/typecheck::infer-contract-method`, `selfhost/typecheck::infer-contract-dispatch`
- `selfhost/typecheck_infer_effect_v1.gc`: `selfhost/typecheck::eff-unknown`, `selfhost/typecheck::merge-eff-rows`, `selfhost/typecheck::infer-effect-pure`, `selfhost/typecheck::infer-effect-bind`, `selfhost/typecheck::infer-effect-bind-program`, `selfhost/typecheck::infer-bind-continuation`, `selfhost/typecheck::finish-effect-bind`, `selfhost/typecheck::infer-effect-perform`
- `selfhost/typecheck_infer_app_v1.gc`: `selfhost/typecheck::infer-app`, `selfhost/typecheck::infer-fallback-app`
- `selfhost/typecheck_typed_effects_v1.gc`: `selfhost/typecheck::effects-empty`, `selfhost/typecheck::effects-merge`, `selfhost/typecheck::effects-with-unknown`, `selfhost/typecheck::effects-add-row`, `selfhost/typecheck::syntax-effects-forms`, `selfhost/typecheck::typed-effects-forms`, `selfhost/typecheck::typed-effects-forms-loop`, `selfhost/typecheck::effects-in-forms-with-env`
- `selfhost/typecheck_unknown_signatures_v1.gc`: `selfhost/typecheck::unknown-signature-symbols`, `selfhost/typecheck::collect-unknown-signatures`, `selfhost/typecheck::collect-unknown-signatures-list`, `selfhost/typecheck::collect-unknown-special`, `selfhost/typecheck::collect-unknown-fn`, `selfhost/typecheck::bind-symbols`, `selfhost/typecheck::bind-symbols-loop`, `selfhost/typecheck::collect-unknown-let`
- `selfhost/typecheck_package_meta_v1.gc`: `selfhost/typecheck::path-message`, `selfhost/typecheck::map-vec-push`, `selfhost/typecheck::symbol-vector`, `selfhost/typecheck::symbol-vector-loop`, `selfhost/typecheck::meta-exports`, `selfhost/typecheck::meta-caps-result`, `selfhost/typecheck::meta-types-result`, `selfhost/typecheck::meta-bool-result`
- `selfhost/typecheck_module_profile_descriptor_v1.gc`: `selfhost/typecheck::module-resolution-profile`, `selfhost/typecheck::module-required-profiles`, `selfhost/typecheck::module-required-profiles-message`, `selfhost/typecheck::map-has-key?`, `selfhost/typecheck::map-has-key-loop`, `selfhost/typecheck::module-error-add`, `selfhost/typecheck::module-profile-active?`, `selfhost/typecheck::module-profile-active-loop`
- `selfhost/typecheck_module_profile_references_v1.gc`: `selfhost/typecheck::collect-module-references`, `selfhost/typecheck::collect-module-references-loop`, `selfhost/typecheck::collect-references`, `selfhost/typecheck::symbol-keyword?`, `selfhost/typecheck::collect-pair-references`, `selfhost/typecheck::collect-fn-references`, `selfhost/typecheck::collect-let-references`, `selfhost/typecheck::collect-let-binding-references`
- `selfhost/typecheck_module_profile_resolution_v1.gc`: `selfhost/typecheck::module-content-identity`, `selfhost/typecheck::module-paths`, `selfhost/typecheck::module-paths-loop`, `selfhost/typecheck::module-identities`, `selfhost/typecheck::module-identities-loop`, `selfhost/typecheck::validate-module-paths`, `selfhost/typecheck::validate-module-paths-loop`, `selfhost/typecheck::add-duplicate-path-errors`
- `selfhost/typecheck_profile_negotiation_v1.gc`: `selfhost/typecheck::profile-negotiation-id`, `selfhost/typecheck::profile-families`, `selfhost/typecheck::profile-lineages`, `selfhost/typecheck::core-profile-offer`, `selfhost/typecheck::profile-negotiation-active?`, `selfhost/typecheck::profile-negotiation-active-loop`, `selfhost/typecheck::profile-errors-add-vector`, `selfhost/typecheck::profile-errors-add-vector-loop`
- `selfhost/typecheck_contract_profile_v1.gc`: `selfhost/typecheck::contract-composition-profile`, `selfhost/typecheck::contract-profile-active?`, `selfhost/typecheck::contract-profile-active-loop`, `selfhost/typecheck::contract-require-profiles`, `selfhost/typecheck::contract-require-true`, `selfhost/typecheck::contract-parse-exports`, `selfhost/typecheck::contract-parse-exports-loop`, `selfhost/typecheck::contract-parse-exports-items`
- `selfhost/typecheck_contract_profile_compose_v1.gc`: `selfhost/typecheck::compose-contract-profile`, `selfhost/typecheck::compose-contract-modules`, `selfhost/typecheck::compose-contract-module`, `selfhost/typecheck::entries-key-set`, `selfhost/typecheck::entries-key-set-loop`, `selfhost/typecheck::compose-contract-exports`, `selfhost/typecheck::compose-contract-exports-loop`, `selfhost/typecheck::map-set-add`
- `selfhost/typecheck_package_context_v1.gc`: `selfhost/typecheck::package-context-empty`, `selfhost/typecheck::package-error-add`, `selfhost/typecheck::collect-package-context`, `selfhost/typecheck::collect-package-context-loop`, `selfhost/typecheck::collect-module-context`, `selfhost/typecheck::collect-export-context-loop`, `selfhost/typecheck::collect-export-context`, `selfhost/typecheck::finalize-package-context`
- `selfhost/typecheck_package_module_v1.gc`: `selfhost/typecheck::state-error`, `selfhost/typecheck::state-warning`, `selfhost/typecheck::state-path-error`, `selfhost/typecheck::state-path-warning`, `selfhost/typecheck::module-report-base`, `selfhost/typecheck::prefix-package-errors`, `selfhost/typecheck::prefix-package-errors-loop`, `selfhost/typecheck::typecheck-module`
- `selfhost/typecheck_package_exports_v1.gc`: `selfhost/typecheck::check-module-exports`, `selfhost/typecheck::check-export-effects-loop`, `selfhost/typecheck::check-export-effects`, `"`, `selfhost/typecheck::validate-export-effects`, `selfhost/typecheck::validate-effect-caps-loop`, `selfhost/typecheck::check-export-types-loop`, `selfhost/typecheck::check-export-type`
- `selfhost/typecheck_package_report_v1.gc`: `selfhost/typecheck::module-state-to-report`, `selfhost/typecheck::merge-context-errors`, `selfhost/typecheck::merge-context-errors-loop`, `selfhost/typecheck::typecheck-package-modules`, `selfhost/typecheck::typecheck-package-modules-loop`, `selfhost/typecheck::merge-module-report`, `selfhost/typecheck::module-diagnostics`, `selfhost/typecheck::diagnostics-loop`
- `selfhost/patch_schema_v1.gc`: `selfhost/patch_schema::is-int?`, `selfhost/patch_schema::is-str?`, `selfhost/patch_schema::is-sym?`, `selfhost/patch_schema::is-sym-or-str?`, `selfhost/patch_schema::is-vec?`, `selfhost/patch_schema::is-map?`, `selfhost/patch_schema::err`, `selfhost/patch_schema::require`
- `selfhost/patch_schema_apply_v1.gc`: `selfhost/patch_schema::vec-replace-loop`, `selfhost/patch_schema::step-tag`, `selfhost/patch_schema::apply-replace-step-sym`, `selfhost/patch_schema::apply-replace-step`, `selfhost/patch_schema::apply-replace-term`, `core/cli::apply-replace-node`, `core/cli::print-module-forms`, `core/cli::canonicalize-module-content`
- `selfhost/patch_schema_manifest_v1.gc`: `selfhost/patch_schema::empty-vec`, `selfhost/patch_schema_manifest::err`, `selfhost/patch_schema::str?`, `selfhost/patch_schema::sym?`, `selfhost/patch_schema::vec?`, `selfhost/patch_schema::map?`, `selfhost/patch_schema::key->str`, `selfhost/patch_schema::require-map-field`
- `selfhost/patch_schema_refactor_v1.gc`: `selfhost/patch_refactor::is-error`, `selfhost/patch_refactor::err`, `selfhost/patch_refactor::empty-vec`, `selfhost/patch_refactor::vec2`, `selfhost/patch_refactor::res`, `selfhost/patch_refactor::res-term`, `selfhost/patch_refactor::res-count`, `selfhost/patch_refactor::rename-result`
- `selfhost/patch_schema_refactor_meta_migrate_v1.gc`: `selfhost/patch_refactor::vec-filter-remove`, `selfhost/patch_refactor::vec-filter-remove2`, `selfhost/patch_refactor::vec-str->sym-vec`, `selfhost/patch_refactor::vec-str->sym-vec2`, `selfhost/patch_refactor::rewrite-meta-field`, `selfhost/patch_refactor::rewrite-meta-forms-loop`, `selfhost/patch_refactor::rewrite-meta-list-apply`, `core/cli::rewrite-meta-list-forms`
- `selfhost/patch_authority_identity_v1.gc`: `selfhost/patch_authority::PROFILE`, `selfhost/patch_authority::REQUEST_KIND`, `selfhost/patch_authority::REPORT_KIND`, `selfhost/patch_authority::NODE_ID_PREFIX`, `selfhost/patch_authority::err`, `selfhost/patch_authority::vec1`, `selfhost/patch_authority::vec2`, `selfhost/patch_authority::vec4`
- `selfhost/patch_authority_normalize_v1.gc`: `selfhost/patch_normalize::REQUEST_KIND`, `selfhost/patch_normalize::REPORT_KIND`, `selfhost/patch_normalize::err`, `selfhost/patch_normalize::has-key-loop`, `selfhost/patch_normalize::has-key`, `selfhost/patch_normalize::vec-has`, `selfhost/patch_normalize::keys-allowed-loop`, `selfhost/patch_normalize::keys-required-loop`
- `selfhost/patch_authority_preflight_v1.gc`: `selfhost/patch_preflight::REQUEST_KIND`, `selfhost/patch_preflight::REPORT_KIND`, `selfhost/patch_preflight::err`, `selfhost/patch_preflight::valid-state?`, `selfhost/patch_preflight::state-record`, `selfhost/patch_preflight::build-state`, `selfhost/patch_preflight::state-records-loop`, `selfhost/patch_preflight::state-records`
- `selfhost/patch_authority_refactor_plan_v1.gc`: `selfhost/refactor_plan::REQUEST_KIND`, `selfhost/refactor_plan::REPORT_KIND`, `selfhost/refactor_plan::PROFILE`, `selfhost/refactor_plan::err`, `selfhost/refactor_plan::conflict`, `selfhost/refactor_plan::push-conflict`, `selfhost/refactor_plan::kind-valid?`, `selfhost/refactor_plan::move-kind?`
- `selfhost/patch_authority_diff_v1.gc`: `selfhost/patch_diff::REQUEST_KIND`, `selfhost/patch_diff::REPORT_KIND`, `selfhost/patch_diff::PROFILE`, `selfhost/patch_diff::err`, `selfhost/patch_diff::has-key-loop`, `selfhost/patch_diff::has-key`, `selfhost/patch_diff::workspace-map-loop`, `selfhost/patch_diff::workspace-map`
- `selfhost/patch_authority_merge_v1.gc`: `selfhost/patch_merge::REQUEST_KIND`, `selfhost/patch_merge::REPORT_KIND`, `selfhost/patch_merge::PROFILE`, `selfhost/patch_merge::err`, `selfhost/patch_merge::optional-eq?`, `selfhost/patch_merge::optional-term`, `selfhost/patch_merge::conflict-core`, `selfhost/patch_merge::conflict`
- `selfhost/patch_authority_apply_report_v1.gc`: `selfhost/patch_apply_report::REQUEST_KIND`, `selfhost/patch_apply_report::REPORT_KIND`, `selfhost/patch_apply_report::PROFILE`, `selfhost/patch_apply_report::err`, `selfhost/patch_apply_report::bool?`, `selfhost/patch_apply_report::hash?`, `selfhost/patch_apply_report::edits-valid-loop`, `selfhost/patch_apply_report::edits-valid?`
- `selfhost/policy_authority_v1.gc`: `selfhost/policy::error`, `selfhost/policy::failure`, `selfhost/policy::map?`, `selfhost/policy::vec?`, `selfhost/policy::str?`, `selfhost/policy::sym?`, `selfhost/policy::int?`, `selfhost/policy::nil?`
- `selfhost/effect_policy_crypto_v1.gc`: `selfhost/effect-crypto::map-has-key-loop?`, `selfhost/effect-crypto::map-has-keys-loop?`, `selfhost/effect-crypto::exact-map?`, `selfhost/effect-crypto::invalid-type?`, `selfhost/effect-crypto::invalid-entry?`, `selfhost/effect-crypto::list-input-loop?`, `selfhost/effect-crypto::list-input?`, `selfhost/effect-crypto::limit-input?`
- `selfhost/effect_policy_network_v1.gc`: `selfhost/effect-network::map-has-key-loop?`, `selfhost/effect-network::map-has-keys-loop?`, `selfhost/effect-network::exact-map?`, `selfhost/effect-network::invalid-type?`, `selfhost/effect-network::invalid-entry?`, `selfhost/effect-network::list-input-loop?`, `selfhost/effect-network::list-input?`, `selfhost/effect-network::optional-bool-input?`
- `selfhost/effect_policy_plugin_v1.gc`: `selfhost/effect-plugin::map-has-key-loop?`, `selfhost/effect-plugin::map-has-keys-loop?`, `selfhost/effect-plugin::exact-map?`, `selfhost/effect-plugin::invalid-type?`, `selfhost/effect-plugin::invalid-entry?`, `selfhost/effect-plugin::list-input-loop?`, `selfhost/effect-plugin::list-input?`, `selfhost/effect-plugin::input-valid?`
- `selfhost/effect_policy_ffi_v1.gc`: `selfhost/effect-ffi::fields`, `selfhost/effect-ffi::input-valid?`, `selfhost/effect-ffi::empty-input`, `selfhost/effect-ffi::signed-error`, `selfhost/effect-ffi::signed-valid`, `selfhost/effect-ffi::state-status?`, `selfhost/effect-ffi::hex64?`, `selfhost/effect-ffi::signed-policy`
- `selfhost/effect_policy_authority_v1.gc`: `selfhost/effect-policy::optional-bool?`, `selfhost/effect-policy::optional-int?`, `selfhost/effect-policy::optional-str?`, `selfhost/effect-policy::map-has-key-loop?`, `selfhost/effect-policy::map-has-key?`, `selfhost/effect-policy::map-has-keys-loop?`, `selfhost/effect-policy::exact-map?`, `selfhost/effect-policy::optional-nonnegative-int?`
- `selfhost/obligation_authority_core_v1.gc`: `selfhost/obligation::REQUEST_KIND`, `selfhost/obligation::RESULT_KIND`, `selfhost/obligation::error`, `selfhost/obligation::tag?`, `selfhost/obligation::map?`, `selfhost/obligation::vec?`, `selfhost/obligation::str?`, `selfhost/obligation::sym?`
- `selfhost/obligation_authority_typecheck_v1.gc`: `selfhost/obligation::strict-typecheck-meta`, `selfhost/obligation::typecheck-modules-loop`, `selfhost/obligation::typecheck`
- `selfhost/obligation_authority_determinism_v1.gc`: `selfhost/obligation::determinism-caps-symbols-loop`, `selfhost/obligation::determinism-module-caps`, `selfhost/obligation::determinism-pure-module?`, `selfhost/obligation::debug-escape-symbol-bytes-loop`, `selfhost/obligation::debug-symbol`, `selfhost/obligation::debug-symbols-loop`, `selfhost/obligation::debug-symbol-set`, `selfhost/obligation::determinism-static-error`
- `selfhost/obligation_authority_lint_v1.gc`: `selfhost/obligation::artifact-hash-term`, `selfhost/obligation::map-has-key-loop?`, `selfhost/obligation::map-has-key?`, `selfhost/obligation::lint-find-meta-loop`, `selfhost/obligation::lint-symbol-exports-loop`, `selfhost/obligation::lint-fill-types-loop`, `selfhost/obligation::lint-autofix-patch`, `selfhost/obligation::lint-autofix-types`
- `selfhost/obligation_authority_ai_style_v1.gc`: `selfhost/obligation::style-strict-code?`, `selfhost/obligation::style-level`, `selfhost/obligation::style-autofix-loop`, `selfhost/obligation::style-fixes`, `selfhost/obligation::style-diagnostic-step`, `selfhost/obligation::style-diagnostics-loop`, `selfhost/obligation::style-modules-loop`, `selfhost/obligation::style-patch-intents-loop`
- `selfhost/obligation_authority_preflight_v1.gc`: `selfhost/obligation::preflight-module-valid?`, `selfhost/obligation::preflight-missing-pin-error`, `selfhost/obligation::preflight-mismatch-error`, `selfhost/obligation::preflight-module-errors`, `selfhost/obligation::preflight-modules-loop`, `selfhost/obligation::preflight-errors`, `selfhost/obligation::preflight`
- `selfhost/obligation_authority_replay_v1.gc`: `selfhost/obligation::replay-map-has-key?`, `selfhost/obligation::replay-map-has-key-loop?`, `selfhost/obligation::replay-entry-valid?`, `selfhost/obligation::replay-entries-valid-loop?`, `selfhost/obligation::replay-observation-valid?`, `selfhost/obligation::replay-starts-with?`, `selfhost/obligation::replay-task-op?`, `selfhost/obligation::replay-task-id-required?`
- `selfhost/obligation_authority_property_v1.gc`: `selfhost/obligation::property-map-has-key-loop?`, `selfhost/obligation::property-map-has-key?`, `selfhost/obligation::property-exact-map?`, `selfhost/obligation::property-keys-loop`, `selfhost/obligation::property-optional-str?`, `selfhost/obligation::property-str4`, `selfhost/obligation::property-str5`, `selfhost/obligation::property-str6`
- `selfhost/obligation_authority_stage_v1.gc`: `selfhost/obligation::stage-eval-valid?`, `selfhost/obligation::stage-optimizer-valid?`, `selfhost/obligation::stage-module-valid?`, `selfhost/obligation::stage-module-errors`, `selfhost/obligation::stage-prefix-errors-loop`, `selfhost/obligation::stage-prefix-errors`, `selfhost/obligation::stage-module-report`, `selfhost/obligation::stage-modules-loop`
- `selfhost/obligation_authority_coverage_v1.gc`: `selfhost/obligation::coverage-profile-valid?`, `selfhost/obligation::coverage-structural?`, `selfhost/obligation::coverage-mcdc?`, `selfhost/obligation::coverage-name`, `selfhost/obligation::coverage-operation`, `selfhost/obligation::coverage-count-row-valid?`, `selfhost/obligation::coverage-site-row-valid?`, `selfhost/obligation::coverage-decision-counts-valid?`
- `selfhost/obligation_authority_translation_v1.gc`: `selfhost/obligation::translation-vector-valid-loop?`, `selfhost/obligation::translation-vector-valid?`, `selfhost/obligation::translation-string-vector-valid?`, `selfhost/obligation::translation-status-valid?`, `selfhost/obligation::translation-value-kind-valid?`, `selfhost/obligation::translation-optional-bool?`, `selfhost/obligation::translation-rewrite-valid?`, `selfhost/obligation::translation-stage2-valid?`
- `selfhost/obligation_authority_gfx_api_v1.gc`: `selfhost/obligation::gfx-api-symbol-vector-valid-loop?`, `selfhost/obligation::gfx-api-symbol-vector-valid?`, `selfhost/obligation::gfx-api-definition-valid?`, `selfhost/obligation::gfx-api-definition-error`, `selfhost/obligation::gfx-api-definitions-loop`, `selfhost/obligation::gfx-api-starts-with?`, `selfhost/obligation::gfx-api-filter-exports-loop`, `selfhost/obligation::gfx-api-filter-exports`
- `selfhost/obligation_authority_gfx_runtime_v1.gc`: `selfhost/obligation::gfx-field-valid?`, `selfhost/obligation::gfx-hash-value`, `selfhost/obligation::gfx-optional-hash-value`, `selfhost/obligation::gfx-pixel-value`, `selfhost/obligation::gfx-kind-value`, `selfhost/obligation::gfx-golden-entry-values`, `selfhost/obligation::gfx-golden-entry-decision`, `selfhost/obligation::gfx-frame-entry-decision`
- `selfhost/obligation_authority_gfx_runtime_finalize_v1.gc`: `selfhost/obligation::gfx-typed-map?`, `selfhost/obligation::gfx-extract-frame`, `selfhost/obligation::gfx-extract-scene`, `selfhost/obligation::gfx-outcome-runtime-error`, `selfhost/obligation::gfx-render-result`, `selfhost/obligation::gfx-case-errors`, `selfhost/obligation::gfx-golden-case-outcome`, `selfhost/obligation::gfx-golden-finalize-loop`
- `selfhost/obligation_authority_v1.gc`: `selfhost/obligation::bind-result`, `selfhost/obligation::dispatch`, `core/cli::obligation-authority`
- `selfhost/stage1_v1.gc`: `selfhost/stage1::is-error`, `selfhost/stage1::type-error`, `selfhost/stage1::bad-form`, `selfhost/stage1::tag`, `selfhost/stage1::empty-vec`, `selfhost/stage1::vec1`, `selfhost/stage1::vec2`, `selfhost/stage1::vec3`
