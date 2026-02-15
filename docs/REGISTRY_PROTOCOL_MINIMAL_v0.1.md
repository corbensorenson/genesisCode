# Remote Registry Minimal Protocol v0.1

HTTP+JSON control messages, binary artifact payloads.

## 1.0 Goals

- content-addressed artifact storage
- refs (name -> commit hash)
- push/pull for artifacts and refs
- integrity checks, chunking, resumability

All network calls are effects and must be logged.

## 2.0 Transport

Base URL: `https://registry.example.com/v1/`

Responses use JSON errors on failures.

## 3.0 Hashing

- Hash algorithm: BLAKE3-256
- Every upload/download must verify hash matches bytes.

## 4.0 Endpoints

### 4.1 Ping

`GET /v1/ping`

### 4.2 Batch has

`POST /v1/store/has` body `{ "hashes": [...] }`

### 4.3 Get

`GET /v1/store/get/<hash>` returns raw bytes

### 4.4 Put (small)

`PUT /v1/store/put/<hash>` body raw bytes (idempotent)

### 4.5 Chunked upload

- `POST /v1/store/upload/start`
- `PUT /v1/store/upload/chunk/<upload_id>/<index>`
- `POST /v1/store/upload/finish`
- optional status endpoint

### 4.6 Refs

- `GET /v1/refs/get?name=...`
- `GET /v1/refs/list?prefix=...`
- `POST /v1/refs/set` with optimistic concurrency (`expected_old`)

Server must enforce policy checks for `refs/set`.

## 5.0 Client algorithms

### 5.1 Pull

- resolve ref
- fetch commit
- fetch result snapshot, patch, required evidence
- fetch snapshot member closure

### 5.2 Push

- compute reachable required set
- batch `store/has`
- upload missing
- advance remote ref via `refs/set`
