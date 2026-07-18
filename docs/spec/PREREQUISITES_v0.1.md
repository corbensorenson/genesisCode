> Bundle Entry: `docs/spec/TESTING_BUNDLE_v0.1.md`

# GenesisCode Prerequisite Contract v0.1

## Authority

`genesis.prerequisites.json` is the sole semantic inventory of host tools, supported version constraints, feature profiles, and native SDK envelopes. `rust-toolchain.toml`, `package.json`, `package-lock.json`, and CI setup remain tool-native installation inputs; their overlapping values MUST equal the prerequisite manifest and are checked as mirrors rather than independent authorities.

The Draft 2020-12 representation is defined by `docs/spec/PREREQUISITES_v0.1.schema.json`. `scripts/lib/prerequisite_manifest.py` adds semantic validation that JSON Schema cannot express: sorted unique identities, exact profile membership, safe probe commands, profile/platform compatibility, coherent version ranges, source-mirror equality, and required profile coverage.

R0.3.b defines presence and compatibility only. R0.3.c separately owns dependency acquisition, offline mirrors, and network denial. This contract does not claim that a present tool, SDK, browser, simulator, or device is already mirrored or reproducible.

## Read-only diagnostic

Run the minimum native-development profile:

```sh
bash scripts/genesis_prerequisites.sh --profile core
```

Request deterministic JSON for an agent or gate:

```sh
bash scripts/genesis_prerequisites.sh --profile web --format json
```

List profiles without probing the host:

```sh
python3 scripts/lib/prerequisite_manifest.py list-profiles
```

The command exits `0` when every required check passes, `2` when a required tool or SDK is missing/mismatched, and `3` for an invalid manifest, unknown profile/platform, unsafe probe, or diagnostic failure. Optional gaps are reported but do not make a profile fail. If `python3` itself is absent, the Bash wrapper reports the Python floor and exits `2` without attempting installation.

The diagnostic never runs an installer, package manager mutation, `rustup target add`, network command, shell expression, or manifest-provided arbitrary executable. Manifest command probes must exactly match the implementation's reviewed read-only argv allowlist. Each process receives no stdin, a five-second timeout, bounded output capture, and a stable `C` locale. Reports contain normalized versions and statuses, not host paths or raw command output.

## Profiles

| Profile | Required scope | Native SDK | Notes |
|---|---|---:|---|
| `core` | Bash, Python, Git, Rust/Cargo/rustfmt/Clippy | yes | Default local build/check profile; nextest, jq, and ShellCheck are optional |
| `ci` | core plus exact nextest, cargo-deny, jq | yes | Standard CI and supply-chain profile |
| `web` | Node/npm, locked Playwright, wasm-bindgen, `wasm32-unknown-unknown` | yes | Node, browser, Web, and WebXR lanes |
| `wasi` | WASI SDK 33.0, Wasmtime, and `wasm32-wasip1` | yes | Preview 1 CLI parity, including Rust crates with native C dependencies |
| `formal` | Lean and Lake | no | R7 mechanized semantics/proofs; may run independently of native builds |
| `fuzz` | cargo-fuzz and compatible Clang | yes | R7 fuzz/property campaigns; excludes Windows until a qualified backend is declared |
| `apple-device` | Xcode and xcrun | yes | Darwin only; ios-deploy/libimobiledevice are optional physical-device helpers |
| `android-device` | Android Debug Bridge | yes | Android emulator or physical-device workflows |
| `full` | union of core, CI, web, WASI, and formal | yes | Release-development profile; device/fuzz profiles remain explicit opt-ins |

A profile declares its supported platform IDs. Selecting a profile on an undeclared platform is an error rather than a silent skip. `full` deliberately does not require Android and Apple device stacks simultaneously.

## Version policy

- Rust stage0 is exact `1.90.0`; Cargo and both installed WebAssembly targets must belong to that toolchain. rustfmt and Clippy are exact component versions.
- WASI SDK is exact `33.0`. Its official platform archive is SHA-256 verified by `scripts/install_wasi_sdk.sh`; `WASI_SDK_PATH`, `WASI_SYSROOT`, and target-specific Cargo C compiler variables must identify the same extracted SDK. The Rust target alone is insufficient for crates such as bundled SQLite that compile C sources.
- Python is `>=3.9.0 <4.0.0`; repository helpers must remain valid on Python 3.9 and cannot assume `tomllib`.
- Bash is `>=3.2.0 <6.0.0`, preserving the macOS system Bash floor. Scripts cannot require Bash 4-only features without advancing this profile.
- Node is `>=22.0.0 <23.0.0`, npm is `>=10.0.0 <11.0.0`, Playwright is exact `1.58.2`, and wasm-bindgen CLI is exact `0.2.108`.
- Wasmtime is exact `36.0.9`, the selected maintained release line for current WASI parity. Advancing it requires rerunning WASI, replay, and cross-host gates.
- Lean is exact `4.31.0` and Lake's exposed tool version is exact `5.0.0`. Advancing either requires proof artifact migration evidence once the formal project exists.
- cargo-nextest is exact `0.9.137` and cargo-deny is exact `0.19.0` in CI.
- Range comparisons normalize one-, two-, and three-component numeric versions to three components. Prerelease/vendor suffixes are excluded by probe regex before comparison.

Exact versions are compatibility identities, not claims that newer tools are defective. A newer unqualified version fails closed until its semantic, artifact, resource, and platform effects are reviewed and the manifest is deliberately advanced.

## Platform SDK envelopes

| Platform | Tier | SDK expectation |
|---|---:|---|
| `darwin-arm64` | 1 | Xcode `>=16.2 <17`, macOS SDK `>=15.2 <16`, Apple Clang `>=16 <17` |
| `linux-x86-64` | 1 | discoverable native `cc` toolchain |
| `linux-arm64` | 2 | discoverable native `cc` toolchain |
| `windows-x86-64` | 2 | discoverable MSVC `cl` toolchain |

Platform probes establish only the compiler/SDK envelope. Simulator runtime versions, device images, signing identities, GPU adapters, browsers, and platform packages remain lane-specific evidence and MUST NOT be inferred from a passing prerequisite report.

## Maintenance and negative controls

`scripts/check_prerequisite_manifest.sh` is the read-only conformance gate. It validates schema identity and semantic closure, verifies the tool-native mirrors, checks the current core profile twice for byte-identical reports, and proves the retained contract is unchanged. Negative controls reject:

1. duplicate manifest keys;
2. manifest-selected mutating/arbitrary command argv;
3. Rust/source mirror drift;
4. removal of a required profile tool;
5. unknown profiles; and
6. unsupported profile/platform combinations; and
7. Rust target identity drift from `rust-toolchain.toml`.

Version updates must change the authoritative manifest and every affected tool-native mirror together, explain compatibility impact, and run the feature-specific gates. The diagnostic offers no installation command because installation is host-mutating and belongs to an explicit bootstrap procedure, not a check.
