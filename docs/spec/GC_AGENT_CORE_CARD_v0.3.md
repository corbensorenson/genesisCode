# GC-AGENT-v0.3 Core Card

Training-frozen surface. Reject unlisted or unsupported behavior; do not guess.
Pure evaluation is deterministic. Filesystem, time, network, process, and LLM work only through explicit deny-by-default effects with run/replay equivalence. User input must never panic; boundaries return sealed ERROR values. UNHANDLED, EFFECT, and ERROR are unforgeable.

## Surface
- lexical-grammar: nil true false integer string bytes symbol quote list vector map comment
- coreform-mapping: Nil Bool Int Str Bytes Symbol Pair Vector Map
- evaluation: quote fn if begin let prim seal unseal def application
- values: Data Int Vector Map Closure CompiledClosure SealToken Sealed NativeFn Contract EffectProgram EffectRequest bytes/concat bytes/from-hex bytes/get bytes/join bytes/len bytes/slice bytes/to-hex bytes/to-str-utf8 core/eq? coreform/escape-bytes coreform/escape-str crypto/blake3 data/tag dec/add dec/eq? dec/from-int dec/lt? dec/mul dec/parse dec/sub dec/to-str int/add int/eq? int/lt? int/mul int/sub int/to-str list/is-nil? map/entries map/from-entries map/get map/len map/merge map/put pair/as-proper-list pair/car pair/cdr pair/cons str/concat str/join str/len str/repeat str/to-bytes-utf8 sym/eq? sym/from-str sym/to-str utf8/encode-codepoint vec/get vec/len vec/push vec/set
- contracts: core/contract::genesis core/contract::make core/contract::extend core/contract::dispatch core/contract::explain core/contract::meta core/contract::proto core/contract::shape core/msg::make core/msg::op core/msg::payload
- modules: def ::meta :caps :exports :types module-path::name
- effects: core/effect::pure core/effect::perform core/effect::bind core/effect::map core/effect::then core/effect::catch core/effect::catch-payload caps.toml .gclog replay
- packages: schema name version modules dependencies obligations tests property_tests caps_policy limits budgets property gfx
- errors: BadForm Unbound Type NotCallable Internal StepLimit MemoryLimit UNHANDLED EFFECT ERROR genesis/error-v0.2 genesis/diagnostics-schema-v1 genesis/diagnostic-catalog-v0.1 genesis/diagnostic/v1
- resource-limits: step_limit allow_unlimited max_alloc_units max_live_units max_pair_cells max_vec_len max_map_len max_bytes_len max_string_len max_steps_per_test max_effect_entries_per_test max_effect_log_bytes_per_test max_effect_ops max_payload_bytes_per_op max_payload_bytes_per_run max_response_bytes_per_op max_response_bytes_per_run
- compatibility-identifiers: GC-AGENT-v0.3 genesis/language-profile/v0.2 genesis/coreform/v0.2 genesis/hash-profile/gcv0.2-blake3 genesis/value-effect-hash/v0.2 genesis/effect-log/v3 genesis/error-v0.2 genesis/diagnostics-schema-v1 package-schema-1 genesis-lock-v2 gpk-v2 reserved-not-stable

## Examples
- positive eval-arithmetic => 42: `(prim int/add 20 22)`
- positive eval-lexical-closure => 6: `(let ((x 2)) ((fn (y) (prim int/mul x y)) 3))`
- positive eval-effect-program => EffectProgram: `(core/effect::pure 1)`
- negative eval-unbound-symbol => Unbound: `missing-symbol`
- negative step-limit => StepLimit: `(begin 1 2 3 4)`
- negative vector-limit => MemoryLimit: `[1 2]`

Negative examples are valid syntax that must fail at the named semantic/resource boundary.
Compatibility: agentProfile=GC-AGENT-v0.3 cliEnvelope=genesis/error-v0.2 coreformProfile=genesis/coreform/v0.2 diagnostics=genesis/diagnostics-schema-v1 effectLog=genesis/effect-log/v3 genesisLock=2 gpk=2 hashProfile=genesis/hash-profile/gcv0.2-blake3 languageProfile=genesis/language-profile/v0.2 packageManifest=1 releaseTrain=0.2.0 v1ReleaseClaim=reserved-not-stable valueEffectHashProfile=genesis/value-effect-hash/v0.2
Unsupported classes: experimental-syntax host-only-operation unavailable-target nondeterministic-facility out-of-profile-capability
Authority: docs/spec/GC_AGENT_PROFILE_v0.3.json profile-sha256=c140cf129c3118ae3e4a49c3d103783b63c00e514b005897f7e61366e4941995
Verify: bash scripts/check_gc_agent_core_card.sh
