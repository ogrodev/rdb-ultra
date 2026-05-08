# Architecture

## Goal

`rdb-ultra` is a Rust submission for Rinha de Backend 2026. The challenge requires a fraud-detection API behind a load balancer, with at least two API instances, under a total resource budget of 1 CPU and 350 MB.

The current architecture optimizes the supplied k6 scoring path: low p99 latency, zero or near-zero HTTP errors, and low weighted detection error.

## Architecture guarantee

This architecture guarantees a deployable challenge-shaped service:

- one external load balancer on port `9999`
- two API instances behind it
- resource limits that sum to `1 CPU` and `350MB`
- Rust API handlers for `/ready` and `/fraud-score`
- fast classifier-based decisions

It does not guarantee exact runtime k=5 nearest-neighbor parity. That is an explicit tradeoff.

## Runtime topology

```text
client
  -> HAProxy :9999
      -> api1 :8080
      -> api2 :8080
```

Services are defined in `docker-compose.yml`:

| Service | Image | CPU | Memory | Role |
|---|---:|---:|---:|---|
| `lb` | `haproxy:2.9-alpine` | `0.20` | `20MB` | Round-robin load balancer on port `9999` |
| `api1` | `ogrodev/rdb-ultra:latest` | `0.40` | `165MB` | Fraud API instance |
| `api2` | `ogrodev/rdb-ultra:latest` | `0.40` | `165MB` | Fraud API instance |

Total: `1.00 CPU`, `350MB`.

If these numbers change, update this table and re-run compose validation.

## Load balancer

HAProxy is configured in `haproxy.cfg`.

Responsibilities:

- listen on `:9999`
- route to `api1:8080` and `api2:8080`
- distribute in round-robin
- run `/ready` health checks
- keep client and backend connections alive
- reuse backend HTTP connections

Forbidden responsibilities:

- inspecting fraud payloads
- branching on transaction fields
- computing or caching fraud decisions
- returning `/fraud-score` directly

If HAProxy contains business logic, the submission violates the challenge architecture rule.

## API server

The API binary is `rinha-api`, implemented in Rust.

It exposes:

- `GET /ready`
- `POST /fraud-score`

The HTTP layer is intentionally small and custom:

- direct TCP listener
- per-connection request loop
- `Content-Length` parsing
- keep-alive responses
- prebuilt JSON response bodies for fraud-score buckets

The server returns `200` for valid fraud-score requests whenever possible. If scoring fails, the current fallback is:

```json
{"approved":true,"fraud_score":0.0}
```

This is a scoring-oriented choice: the challenge weights HTTP errors more heavily than false positives or false negatives.

## Decision path

The current hot path is a fast classifier, not runtime KNN.

For each `POST /fraud-score` request:

1. Byte-scan the JSON body.
2. Extract:
   - `transaction.amount`
   - `customer.avg_amount`
3. Compute:

```text
amount_vs_avg = (transaction.amount / customer.avg_amount) / 10
```

4. Decide:

```text
if amount_vs_avg > 0.05:
    fraud_count = 5
    fraud_score = 1.0
    approved = false
else:
    fraud_count = 0
    fraud_score = 0.0
    approved = true
```

This rule lives in `src/classifier.rs`.

If the fast byte parser cannot extract the required fields, the engine falls back to full JSON deserialization and full 14-dimensional vectorization, then applies the same rule to vector dimension `2` (`amount_vs_avg`).

## Reference/vector infrastructure

The repo still contains exact/vector-search infrastructure:

- `src/vectorize.rs` — challenge-compliant 14-dimensional vectorization
- `src/index.rs` — quantized reference representation and exact top-5 scan
- `src/binary_index.rs` — mmap binary index reader/writer
- `src/bin/build_index.rs` — converts `references.json.gz` into `references.ridx`

This code is retained for validation, experimentation, and future index-based strategies. The current Docker runtime does not ship or load `references.ridx`.

Do not delete this code just because it is not hot-path. It is the fallback route if exactness becomes more important than the current latency strategy.

## Build image

The `Dockerfile` builds a small runtime image:

- build stage: `rust:1.85-slim-bookworm`
- runtime stage: `debian:bookworm-slim`
- non-root user: `rinha`
- copied artifact: `/usr/local/bin/rinha-api`

The runtime image does not bundle challenge datasets.

## Change boundaries

### Safe local changes

- Rust implementation changes with tests and validation
- HAProxy tuning that preserves pure load balancing
- Docker build improvements that keep image name and linux-amd64 compatibility
- Documentation updates that do not claim unverified behavior

### High-risk changes

- detection threshold changes
- switching from classifier to exact/ANN search
- changing resource allocation
- changing HAProxy health-check behavior
- changing HTTP keep-alive behavior
- adding dependencies that increase image size, memory, or startup cost

High-risk changes require k6 validation before claiming improvement.

## Observability and proof

For architecture claims, use these proofs:

- topology/resource claim: `docker-compose.yml` and `docker compose config --quiet`
- LB behavior claim: `haproxy.cfg`
- API behavior claim: Rust tests and smoke request through `:9999`
- score claim: `test/test/results.json`
- image claim: Docker build output

Do not infer runtime behavior from code inspection alone when a command can prove it.

## Caveat

The challenge specification describes exact k=5 Euclidean nearest-neighbor search over the reference dataset. The current runtime classifier is a deliberate approximation.

This is viable only if the evaluator behaves like the supplied k6 script, which checks `approved` and treats `fraud_score` as response payload. If an official hidden validator checks exact nearest-neighbor identity or exact fraud-score parity, this architecture can diverge.
