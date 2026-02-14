# `.gclog` (Effect Log) v0.2

Effect logs are stored as a canonical CoreForm term (a map) and are intended to be replayed deterministically.

## Top-Level Term

The log is a map with keys:
- `:version` (int)
- `:program-hash` (bytes[32]): hash of the canonical module/program being run
- `:toolchain` (string): informational toolchain identifier
- `:entries` (vector): ordered list of entry maps

## Entry Term

Each entry is a map with keys:
- `:i` (int): 0-based index
- `:op` (symbol): effect op symbol
- `:payload-h` (bytes[32]): hash of the request payload datum
- `:cont-h` (bytes[32]): hash of the continuation closure/value
- `:req-h` (bytes[32]): request hash, derived from `:op`, `:payload-h`, `:cont-h`
- `:decision` (symbol): `:allow` or `:deny`
- `:cap` (term): capability policy details used for the decision (or `nil`)
- `:resp` (map): tagged response payload
- `:resp-h` (bytes[32]): hash of the replayed response value

Response tag map:
- `:kind` is `:ok` or `:error`
- `:value` is the response datum payload

## Normative Replay Rules

- Replay must consume exactly all entries. Missing/extra entries fail.
- Replay must re-compute `:payload-h`, `:cont-h`, `:req-h` and compare.
- Replay must reconstruct the response value from `:resp`, hash it, and compare to `:resp-h`.
- Any mismatch is a hard failure.

