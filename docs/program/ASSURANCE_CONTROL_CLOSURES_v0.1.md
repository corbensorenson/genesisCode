# Assurance Control Closures v0.1

Last updated: 2026-02-24

Scope:
- Signed closure bundles for governance controls previously tracked as `program-backlog`.
- Immutable reference map used by crosswalk validation and readiness gates.

Canonical schema:
- `docs/program/ASSURANCE_CONTROL_CLOSURES_v0.1.json`

## Closed Controls

| Control ID | Status | Closure bundle | Bundle SHA-256 | Signed summary SHA-256 |
| --- | --- | --- | --- | --- |
| `DO178C-A-ORG-INDEPENDENCE-PROGRAM` | closed | `docs/program/assurance_closures/do178c-a-org-independence-program.closure.json` | `4d821d2aba3430d11cd72dd96e95a1f6a61e79b0eb2bf2502061dc228d225e46` | `7ed3c3313a3f60c78da5892a230179a0553a60152dec9fb51bc5f12ecba49d34` |
| `DO178C-B-AUTHORITY-SIGNOFF` | closed | `docs/program/assurance_closures/do178c-b-authority-signoff.closure.json` | `31533ffae41beeb44b2eb184a4d0d6fd1a2e74597146f4f02d4d5377e2a89a82` | `fb3a60d4d019884f141f00a3dfea23e9614f28395376f6b40d3e5a92bad7a9c1` |
| `IEC62304-C-QMS-INTEGRATION` | closed | `docs/program/assurance_closures/iec62304-c-qms-integration.closure.json` | `68a9f4ee61eeeebbf4b6e1b6acc7be0469af5299e899566f500ea12852a4ee69` | `47003ed9114b43fe95a0d7678a9fe3af5f978630f017909f10833aca597835d2` |
| `NASA-A-IVV-GOVERNANCE` | closed | `docs/program/assurance_closures/nasa-a-ivv-governance.closure.json` | `8ecc14af7289b5a90a9b7291813b0306beea29b3fbe6f8d5f410e17d33059e48` | `24198a9b8c14bca00e88504833ec684ffa8b54ddaa6a9a898a8a3589636db324` |
| `NASA-B-IVV-ORG-SCOPE` | closed | `docs/program/assurance_closures/nasa-b-ivv-org-scope.closure.json` | `5907452a148c32d9ae54574e16b145f5400f214193d31ddb066a40f5d0b98c85` | `92cc13d13473ee577092083aa3b44802610488a85125080b9f808aa417a99e00` |

## Immutable Reference Contract

- Every closure bundle includes `immutable_artifact_refs` with `urn:genesis:artifact:sha256:<digest>` references.
- Crosswalk validation requires each closed control to expose `closure_bundle`, `closure_bundle_sha256`, `signed_summary_sha256`, and `immutable_refs`.
- Closed controls are tracked via anchors in this document and validated against the canonical JSON registry.

### do178c-a-org-independence-program

- Control: `DO178C-A-ORG-INDEPENDENCE-PROGRAM`
- Closure bundle: `docs/program/assurance_closures/do178c-a-org-independence-program.closure.json`
- Bundle SHA-256: `4d821d2aba3430d11cd72dd96e95a1f6a61e79b0eb2bf2502061dc228d225e46`
- Signed summary SHA-256: `7ed3c3313a3f60c78da5892a230179a0553a60152dec9fb51bc5f12ecba49d34`
- Immutable refs:
  - `urn:genesis:artifact:sha256:ef52d232fa65c6d7b8d45566fac421872e7828533b441cf8df067909e10f2965`
  - `urn:genesis:artifact:sha256:ca3baf9ae20c5b830467284154d1b25432961613c4d3c746fdc76490f06138c4`
  - `urn:genesis:artifact:sha256:c9216b17bdd94f4d7459b38b3da11379d5b47589baa9597dd788c0fab46a253a`

### do178c-b-authority-signoff

- Control: `DO178C-B-AUTHORITY-SIGNOFF`
- Closure bundle: `docs/program/assurance_closures/do178c-b-authority-signoff.closure.json`
- Bundle SHA-256: `31533ffae41beeb44b2eb184a4d0d6fd1a2e74597146f4f02d4d5377e2a89a82`
- Signed summary SHA-256: `fb3a60d4d019884f141f00a3dfea23e9614f28395376f6b40d3e5a92bad7a9c1`
- Immutable refs:
  - `urn:genesis:artifact:sha256:d35a17b4d38045d2935f1af01774736651fa6260a7acd32f98564b432165493a`
  - `urn:genesis:artifact:sha256:e7f6fb4d5a7e6e68759a4acd350ffa90e3a2e80c61e567fe213d301116a0b88d`
  - `urn:genesis:artifact:sha256:b979c31dd35a37bd728415832a4c59c9325b97bb93fce3db78f55fbd71e586ee`

### iec62304-c-qms-integration

- Control: `IEC62304-C-QMS-INTEGRATION`
- Closure bundle: `docs/program/assurance_closures/iec62304-c-qms-integration.closure.json`
- Bundle SHA-256: `68a9f4ee61eeeebbf4b6e1b6acc7be0469af5299e899566f500ea12852a4ee69`
- Signed summary SHA-256: `47003ed9114b43fe95a0d7678a9fe3af5f978630f017909f10833aca597835d2`
- Immutable refs:
  - `urn:genesis:artifact:sha256:9cbfb080f9e8c9eff0fcc5465260fbec07a3ffff3eb33e1afba465f9df2b043f`
  - `urn:genesis:artifact:sha256:d29af598abdc9d55576b1b03b5dc380500da38fab9f05f4008b6f0907e218d62`
  - `urn:genesis:artifact:sha256:26e07b9884dbc8bc163ef04368199a4ea4b1595e37ee03bd0d09572b150ccc63`

### nasa-a-ivv-governance

- Control: `NASA-A-IVV-GOVERNANCE`
- Closure bundle: `docs/program/assurance_closures/nasa-a-ivv-governance.closure.json`
- Bundle SHA-256: `8ecc14af7289b5a90a9b7291813b0306beea29b3fbe6f8d5f410e17d33059e48`
- Signed summary SHA-256: `24198a9b8c14bca00e88504833ec684ffa8b54ddaa6a9a898a8a3589636db324`
- Immutable refs:
  - `urn:genesis:artifact:sha256:99bd2c3efc2224f92d0b4cad31d4b832d0725d04c7300290563012a4329938e4`
  - `urn:genesis:artifact:sha256:a1f06edca4679fb297cf6f05533a5e9f67f9faa8afecd9f167ad0b544555ec1f`
  - `urn:genesis:artifact:sha256:6fecda6ab47c048ab064ffb81d2187e02e2edca1f7328ff9837f2909b5a22756`

### nasa-b-ivv-org-scope

- Control: `NASA-B-IVV-ORG-SCOPE`
- Closure bundle: `docs/program/assurance_closures/nasa-b-ivv-org-scope.closure.json`
- Bundle SHA-256: `5907452a148c32d9ae54574e16b145f5400f214193d31ddb066a40f5d0b98c85`
- Signed summary SHA-256: `92cc13d13473ee577092083aa3b44802610488a85125080b9f808aa417a99e00`
- Immutable refs:
  - `urn:genesis:artifact:sha256:9522d816c1f24402d21cffc59c189716e93c6ec1d25564f28be45bc34bdc40be`
  - `urn:genesis:artifact:sha256:4d34e3b49aa2006d9c7e04100bb49b828d9470a844b5a4dc480181e79169a0ed`
  - `urn:genesis:artifact:sha256:34089d0760762b7857749c7f5035f4b3e627cbd3f9f2dc0ff7cac6517b054aab`
