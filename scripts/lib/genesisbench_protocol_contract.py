#!/usr/bin/env python3
"""Closed constants for the GenesisBench v0.1 protocol."""

TOP_KEYS = {
    "kind", "version", "protocolId", "status", "sourceSnapshot",
    "authorities", "contextPolicy", "toolPolicy", "capabilityPolicy",
    "attemptPolicy", "modelDisclosurePolicy", "taskVisibilityPolicy",
    "scoringPolicy", "contaminationPolicy", "trackPolicy", "eligibilityPolicy",
    "selfHosting", "contentIdentitySha256",
}

AUTHORITY_PATHS = {
    "agent-core-card": "docs/spec/GC_AGENT_CORE_CARD_v0.3.json",
    "agent-profile": "docs/spec/GC_AGENT_PROFILE_v0.3.json",
    "agent-task-cards": "docs/spec/GC_AGENT_TASK_CARDS_v0.3.json",
    "agent-task-benchmark": "benchmarks/agent_tasks/v0.1/suite.json",
    "benchmark-run-integration-test": "crates/gc_cli/tests/cli_agent_benchmark_run.rs",
    "benchmark-run-schema": "docs/spec/GC_AGENT_BENCHMARK_RUN_v0.1.schema.json",
    "benchmark-run-verifier": "scripts/lib/gc_agent_benchmark_run.py",
    "benchmark-score-schema": "docs/spec/GC_AGENT_BENCHMARK_SCORE_v0.1.schema.json",
    "benchmark-scoring": "docs/spec/GC_AGENT_BENCHMARK_SCORING_v0.1.json",
    "benchmark-scoring-schema": "docs/spec/GC_AGENT_BENCHMARK_SCORING_v0.1.schema.json",
    "genesis-mcp-catalog-source": "crates/gc_cli_driver/src/mcp/catalog.rs",
    "genesisbench-eligibility-schema": "docs/spec/GENESISBENCH_ELIGIBILITY_v0.1.schema.json",
    "genesisbench-eligibility-verifier": "scripts/lib/genesisbench_eligibility.py",
    "genesisbench-contamination-attestation-schema": "docs/spec/GENESISBENCH_CONTAMINATION_ATTESTATION_v0.1.schema.json",
    "genesisbench-contamination-classifier": "scripts/lib/genesisbench_contamination.py",
    "genesisbench-adaptation-manifest-schema": "docs/spec/GENESISBENCH_ADAPTATION_MANIFEST_v0.1.schema.json",
    "genesisbench-hardware-evidence-schema": "docs/spec/GENESISBENCH_HARDWARE_EVIDENCE_v0.1.schema.json",
    "genesisbench-scaffold-manifest-schema": "docs/spec/GENESISBENCH_SCAFFOLD_MANIFEST_v0.1.schema.json",
    "genesisbench-integration-test": "crates/gc_cli/tests/cli_agent_index.rs",
    "genesisbench-normative-spec": "guides/genesisbench.qmd",
    "genesisbench-profile-schema": "docs/spec/GENESISBENCH_PROTOCOL_v0.1.schema.json",
    "genesisbench-protocol-contract": "scripts/lib/genesisbench_protocol_contract.py",
    "genesisbench-run-binding": "scripts/lib/genesisbench_protocol_run.py",
    "genesisbench-track-contract": "scripts/lib/genesisbench_tracks.py",
    "genesisbench-verifier": "scripts/lib/genesisbench_protocol.py",
    "held-out-evaluation": "docs/spec/GC_AGENT_HELD_OUT_EVALUATION_v0.1.json",
    "model-runner-effect": "docs/spec/GC_AGENT_MODEL_RUNNER_EFFECT_v0.1.json",
}

COMPONENT_SELECTIONS = {
    "benchmark": {
        "includeExact": [
            "docs/spec/GC_AGENT_BENCHMARK_RUN_v0.1.schema.json",
            "docs/spec/GC_AGENT_BENCHMARK_SCORE_v0.1.schema.json",
            "docs/spec/GC_AGENT_BENCHMARK_SCORING_v0.1.json",
            "docs/spec/GC_AGENT_HELD_OUT_EVALUATION_v0.1.json",
            "docs/spec/GC_AGENT_PROFILE_v0.3.json",
        ],
        "includePrefixes": ["benchmarks/agent_tasks/v0.1/"],
        "excludePrefixes": ["benchmarks/agent_tasks/v0.1/references/"],
    },
    "documentation": {
        "includeExact": ["README.md", "llms.txt"],
        "includePrefixes": ["docs/", "guides/", "learn/", "reference/"],
        "excludePrefixes": ["docs/program/held-out-disclosures/"],
    },
    "runtime": {
        "includeExact": [
            "Cargo.lock", "Cargo.toml", "genesis.lock", "rust-toolchain.toml",
        ],
        "includePrefixes": ["crates/", "prelude/", "selfhost/"],
        "excludePrefixes": [],
    },
}

ALLOWED_TOOLS = [
    "apply-patch", "build", "check", "diff", "explain", "format",
    "get-card", "package", "parse", "replay", "run", "search-symbol",
    "session-abort", "session-apply", "session-begin", "session-stage",
    "session-status", "session-test", "test", "verify",
]

CONTEXT_BASE = {
    "assemblyAlgorithm": "sha256-domain-separated-ordered-artifacts-v0.1",
    "authorityOrder": [
        "system-policy", "agent-profile", "task-card",
        "context-pack-or-retrieval-transcript", "task-prompt", "task-inputs",
    ],
    "forbiddenPaths": [
        ".genesis/private/", ".git/",
        "benchmarks/agent_tasks/v0.1/references/",
        "docs/program/held-out-disclosures/",
        "examples/agent_benchmark_reproducibility/candidate/",
        "examples/agent_benchmark_reproducibility/invocation/model-output.txt",
    ],
    "maxArtifactBytes": 1_048_576,
    "maxAssembledBytes": 8_388_608,
    "completeCaptureRequired": True,
    "retrievalTranscriptRequired": True,
    "promptMaySelectAuthority": False,
    "cohortSeparationRequired": True,
}

TOOL_POLICY = {
    "interfaceId": "genesis/mcp-interface-v0.1",
    "mcpProtocolVersion": "2025-11-25",
    "catalogAuthorityId": "genesis-mcp-catalog-source",
    "allowedTools": ALLOWED_TOOLS,
    "allowedInteractionModes": [
        "artifact-response-v0.1", "genesis-mcp-2025-11-25",
    ],
    "transport": "bounded-stdio-json-rpc",
    "providerControlChannel": "separate-from-candidate-tool-authority",
    "arbitraryShellAllowed": False,
    "ambientFilesystemAllowed": False,
    "ambientNetworkAllowed": False,
    "completeTranscriptRequired": True,
    "toolErrorsRecorded": True,
}

CAPABILITY_POLICY = {
    "defaultDecision": "deny",
    "authoritySource": "protocol-plus-case-owned-capability-policy",
    "wildcardsAllowed": False,
    "agentMayBroaden": False,
    "caseMinimumIsCeiling": True,
    "effectLogRequired": True,
    "replayRequired": True,
    "hardResourceBoundsRequired": True,
    "policyMinimizationScored": True,
}

ATTEMPT_POLICY = {
    "rankedMaxAttempts": 1,
    "unrankedMaxAttempts": 16,
    "attemptCountPredeclared": True,
    "selectionRule": "first-and-only-ranked-attempt",
    "allAttemptsRecorded": True,
    "failedAttemptsPublished": True,
    "bestOfNRankedAllowed": False,
    "retryBackoff": "none",
}

MODEL_DISCLOSURE_POLICY = {
    "requiredFields": [
        "decoding", "model-id", "model-revision", "provider-id",
        "provider-kind", "prompt-assembly", "runtime", "secret-policy",
        "tokenizer", "training-cutoff-for-clean-claims",
        "weights-for-local-models",
    ],
    "immutableRevisionRequired": True,
    "weightsRequiredForLocal": True,
    "tokenizerRequired": True,
    "runtimeRequired": True,
    "decodingIntegerEncoded": True,
    "trainingCutoffRequiredForCleanClaims": True,
    "unknownProvenanceDefault": "unknown",
    "secretsForbidden": True,
    "promptRetention": "complete-or-cryptographic-commitment-with-custody",
}

SCORING_POLICY = {
    "authorityId": "benchmark-scoring",
    "qualityScaleBasisPoints": 10_000,
    "deterministicArtifactScoringRequired": True,
    "independentRescoreRequiredForRanking": True,
    "judgeModelPreferenceIncluded": False,
    "modelMetricsIncluded": False,
    "invalidScoreBasisPoints": 0,
    "scoreIdentityRequired": True,
    "dimensionBreakdownPublished": True,
}

CONTAMINATION_POLICY = {
    "labels": [
        "declared-contaminated", "declared-uncontaminated",
        "temporal-clean", "unknown",
    ],
    "evidenceOrder": [
        "known-exposure", "temporal-precommitment",
        "declared-non-exposure", "insufficient-evidence",
    ],
    "defaultLabel": "unknown",
    "knownExposureOverridesAll": "declared-contaminated",
    "publicReferenceLabel": "declared-contaminated",
    "declaredUncontaminatedRequiresAttestation": True,
    "temporalCleanEvidence": {
        "modelImmutableReleaseRequired": True,
        "taskPrecommitAfterModelReleaseRequired": True,
        "activeUndisclosedEpochRequired": True,
        "commitmentAndCustodyRequired": True,
        "trainingCutoffRequired": True,
    },
    "claimMustEqualStrongestSupportedLabel": True,
    "attestationSchemaAuthorityId": "genesisbench-contamination-attestation-schema",
    "newLanguageImpliesClean": False,
}

ELIGIBILITY_POLICY = {
    "decisions": ["invalid", "ranked", "unranked"],
    "rankedRequirements": [
        "attempt-policy-exact", "capability-policy-exact",
        "complete-model-disclosure", "independent-byte-identical-rescore",
        "non-public-oracle", "profile-and-snapshot-valid",
        "strongest-contamination-label", "tool-and-context-policy-exact",
        "track-and-cohort-exact", "valid-closed-run-record",
    ],
    "unrankedReasonCodes": [
        "attempt/multiple", "evidence/incomplete", "model/conformance-fixture",
        "score/not-independently-rescored", "task/public-reference",
        "track/admission-not-open", "track/hardware-evidence-incomplete",
        "track/training-provenance-incomplete", "visibility/practice-only",
    ],
    "invalidReasonCodes": [
        "authority/mismatch", "capability/broadened",
        "contamination/overclaim", "context/oracle-leak", "profile/mismatch",
        "run/invalid", "score/mismatch", "snapshot/mismatch",
        "track/declaration-mismatch", "track/hardware-class-mismatch",
        "track/offline-violation", "track/scaffold-mismatch",
    ],
    "cohortKeys": [
        "attempt-policy-identity", "contamination-label", "context-mode",
        "hardware-class", "interaction-mode", "language-profile-artifact",
        "protocol-identity", "scaffold-identity", "task-epoch",
        "task-visibility", "track",
    ],
    "missingEvidenceDecision": "unranked",
    "historicalResultsMutable": False,
    "silentSuppressionAllowed": False,
}

SELF_HOSTING = {
    "networkRequired": False,
    "mandatoryDependencies": ["git", "python3"],
    "commands": [
        {
            "id": "check-profile",
            "argv": ["python3", "scripts/lib/genesisbench_protocol.py", "--check"],
        },
        {
            "id": "classify-run",
            "argv": [
                "python3", "scripts/lib/genesisbench_protocol.py", "--check",
                "--run", "RUN.json", "--attestation", "ATTESTATION.json",
                "--json",
            ],
        },
        {
            "id": "validate-run",
            "argv": [
                "python3", "scripts/lib/gc_agent_benchmark_run.py", "--check",
                "--run", "RUN.json",
            ],
        },
        {
            "id": "rescore",
            "argv": [
                "python3", "scripts/lib/gc_agent_scoring.py", "--score", "--case",
                "CASE", "--candidate", "CANDIDATE", "--genesis-bin", "GENESIS",
                "--selfhost-artifact", "ARTIFACT",
            ],
        },
    ],
    "eligibilitySchemaAuthorityId": "genesisbench-eligibility-schema",
    "canonicalOutput": "ascii-json-sorted-keys-newline",
    "absolutePathsAllowed": False,
    "updateDuringCheckAllowed": False,
}
