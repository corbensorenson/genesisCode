# Remote Registry Minimal Protocol v0.1

Push/pull message formats, chunking, integrity, and refs.

Goal: a first usable Genesis registry: content-addressed storage + refs + push/pull, with integrity
checks and a migration path to stronger security.

## 1.0 Goals

- content-addressed artifact storage
- refs (name -> commit hash)
- push/pull for artifacts and refs
- integrity checks, chunking, resumability
- minimal auth hooks (optional)

All network calls are effects, producing effect logs.

## 2.0 Transport

HTTP (1.1 or 2), JSON control messages, raw binary artifact payloads.

Base URL:

- `https://registry.example.com/v1/`

Headers:

- `X-Genesis-Version: 0.1`

## 3.0 Hashing and integrity

- canonical hash algorithm: BLAKE3-256
- hash encoding: hex or base32 (tooling must agree)
- every artifact upload/download validates hash of bytes

## 4.0 Endpoints (minimal set)

### 4.1 Ping

`GET /v1/ping`

Response:

```json
{
  "ok": true,
  "version": "0.1",
  "hash": "blake3-256",
  "max_chunk_bytes": 4194304
}
```

### 4.2 Batch `has`

`POST /v1/store/has`

Request:

```json
{ "hashes": ["h:....", "h:...."] }
```

Response:

```json
{ "present": { "h:....": true, "h:....": false } }
```

### 4.3 Download artifact

`GET /v1/store/get/<hash>`

Response: raw bytes.

Client must stream to disk, compute hash, and verify equals `<hash>`.

### 4.4 Upload artifact (small)

`PUT /v1/store/put/<hash>`

Body: raw bytes.

Server must stream, compute hash, verify equals `<hash>`, store idempotently.

### 4.5 Upload artifact (chunked)

#### 4.5.1 Start

`POST /v1/store/upload/start`

Request:

```json
{ "hash": "h:....", "size_bytes": 12345678 }
```

Response:

```json
{ "upload_id": "u_abc123", "chunk_bytes": 4194304 }
```

#### 4.5.2 Upload chunk

`PUT /v1/store/upload/chunk/<upload_id>/<index>`

Body: raw bytes.

Response:

```json
{ "ok": true, "received": 4194304 }
```

#### 4.5.3 Finish

`POST /v1/store/upload/finish`

Request:

```json
{ "upload_id": "u_abc123" }
```

Response:

```json
{ "ok": true }
```

Server assembles chunks, computes hash, verifies, stores.

#### 4.5.4 Resume (optional)

`GET /v1/store/upload/status/<upload_id>`

Response:

```json
{ "received_chunks": [0,1,2] }
```

## 5.0 Refs endpoints

### 5.1 Get ref

`GET /v1/refs/get?name=<urlencoded>`

Response:

```json
{ "name": "refs/heads/main", "hash": "h:...." }
```

### 5.2 List refs

`GET /v1/refs/list?prefix=<urlencoded>`

Response:

```json
{ "refs": [{"name":"refs/heads/main","hash":"h:..."}, ...] }
```

### 5.3 Set ref (policy-gated, CAS)

`POST /v1/refs/set`

Request:

```json
{
  "name": "refs/heads/main",
  "hash": "h:....",
  "policy": "policy:default-v0.1",
  "expected_old": "h:...."
}
```

Response:

```json
{ "ok": true, "name": "...", "hash": "h:...." }
```

Rules:

- `expected_old` implements optimistic concurrency. If mismatch: 409 with current head.
- server must enforce: commit exists; required obligations/evidence exist; signatures if required.

## 6.0 Push/Pull algorithms (client-side)

### 6.1 Pull

To fetch a package at ref/commit:

1. resolve ref (if needed): `refs/get`
2. ensure commit + reachable artifacts exist locally:
   - fetch commit
   - read commit to find snapshots/patches/evidence
   - fetch those
   - recursively fetch member hashes from snapshot
3. batch with `store/has` to avoid redundant downloads

### 6.2 Push

To publish a commit:

1. compute required artifact set (commit, snapshots, patches, required evidence)
2. batch `store/has` to find missing set
3. upload missing artifacts
4. call `refs/set` with policy and `expected_old`

## 7.0 Security knobs (minimal)

Optional auth:

- `Authorization: Bearer <token>`

At minimum, server enforces hash integrity. Signature/attestation verification may start client-side
and be enforced server-side later.

## 8.0 Error responses

Standard JSON error:

```json
{ "error": { "code": "not_found", "message": "...", "details": {} } }
```

HTTP codes:

- 400 bad request
- 401 unauthorized
- 403 forbidden
- 404 not found
- 409 conflict
- 413 too large
- 500 internal

