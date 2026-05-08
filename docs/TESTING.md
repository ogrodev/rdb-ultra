# Testing

This document defines what evidence is required for each type of claim. Passing one validator only proves the property named by that validator.

## Required local assets

The challenge repository provides the large assets used for local validation:

- `resources/references.json.gz`
- `test/test-data.json`
- `test/test.js`
- `test/smoke.js`

In this repo, large downloaded/generated artifacts are intentionally ignored by git:

- `resources/references.json.gz`
- `resources/references.ridx`
- `test/test-data.json`
- `test/test/results.json`

Tracked challenge config files:

- `resources/mcc_risk.json`
- `resources/normalization.json`
- `resources/example-payloads.json`

If a validation step needs a missing ignored asset, report the missing path and do not claim that validation passed.

## Proof matrix

| Claim | Required command/evidence | What it proves | What it does not prove |
|---|---|---|---|
| Formatting is clean | `cargo fmt --check` | Rust formatting matches rustfmt | Behavior, score, Docker readiness |
| Rust tests pass | `cargo test` | Unit/integration tests pass | k6 score, official performance |
| Lint is clean | `cargo clippy --all-targets -- -D warnings` | No clippy warnings in checked targets | Runtime behavior |
| Compose is syntactically valid | `docker compose config --quiet` | Compose config parses | Services start, score, image exists remotely |
| Local image builds | `docker build -t ogrodev/rdb-ultra:latest .` | Local image can be built for current platform | linux-amd64 publication readiness |
| linux-amd64 image builds | `docker buildx build --platform linux/amd64 -t ogrodev/rdb-ultra:amd64 --load .` | amd64 image can be built locally | Image is pushed/public |
| Stack is reachable | `docker compose up -d` then `GET /ready` through `:9999` | LB and APIs can serve readiness locally | Fraud score quality |
| Local challenge score | `cd test && k6 run test.js`, then inspect `test/test/results.json` | Local supplied-script score | Official score guarantee |

## Standard Rust validation

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Run this after Rust source changes. If any command fails, fix the source cause and re-run the failed command. Re-run the full sequence before claiming Rust readiness.

## Docker validation

Build local image:

```sh
docker build -t ogrodev/rdb-ultra:latest .
```

Build linux-amd64 image locally:

```sh
docker buildx build --platform linux/amd64 -t ogrodev/rdb-ultra:amd64 --load .
```

Validate compose syntax:

```sh
docker compose config --quiet
```

Start stack:

```sh
docker compose up -d
```

Check readiness through the load balancer:

```sh
curl -i http://localhost:9999/ready
```

Stop stack:

```sh
docker compose down
```

## k6 challenge validation

The challenge k6 script is under `test/test.js` and expects `test/test-data.json` in the same directory.

Run:

```sh
cd test
k6 run test.js
```

The result is written to:

```text
test/test/results.json
```

Always inspect the result file before reporting score. Required fields to report:

- `p99`
- `false_positive_detections`
- `false_negative_detections`
- `http_errors`
- `failure_rate`
- `weighted_errors_E`
- `final_score`

## Latest observed local result

One observed local run after switching to the classifier path produced:

```json
{
  "p99": "2.88ms",
  "false_positive_detections": 1247,
  "false_negative_detections": 0,
  "http_errors": 1,
  "final_score": 3246.71
}
```

Treat this only as an observed local run. Local Docker, CPU scheduling, and official runner conditions may differ.

## Change-specific validators

### Documentation-only changes

Required:

- re-read changed Markdown files
- confirm referenced paths exist or intentionally point to ignored/downloaded assets
- `git status --short` to ensure only intended docs changed

Rust/Docker/k6 are not required unless the docs claim fresh runtime behavior.

### Vectorization changes

Required:

- add or update tests for affected dimensions
- run standard Rust validation
- if behavior changes can affect score, run k6

### Detection/classifier changes

Required:

- add or update tests for threshold/bucket behavior
- run standard Rust validation
- run k6 with `test/test.js`
- compare `p99`, FP, FN, HTTP errors, and final score against the previous observed result

Do not tune production logic from `test/test-data.json`. It may be used to evaluate local behavior, not to generate lookup tables, train models, or derive special cases.

### HTTP/HAProxy/compose changes

Required:

- run standard Rust validation if Rust changed
- run `docker compose config --quiet`
- rebuild image if Docker/Rust changed
- start stack and verify `/ready` through `localhost:9999`
- run k6 if latency, connection handling, resource limits, or HAProxy settings changed

### Dockerfile/image publication changes

Required:

- local Docker build
- linux-amd64 Docker build
- confirm image name in `docker-compose.yml`
- confirm no large ignored challenge assets are staged

## Failure handling

- HTTP errors in k6 are severe; investigate before optimizing detection accuracy.
- p99 regressions require evidence: compare result files, not subjective speed.
- If HAProxy marks backends down, inspect health-check intervals, backend saturation, resource limits, and API process exits.
- If an API exits with code `137`, treat it as likely memory or cgroup pressure until disproven.
- If local assets are missing, fetch them from the challenge repository before running score validation, or explicitly state that k6 validation is blocked.

## Index-building tools

The repo retains a binary index builder for experimentation:

```sh
cargo run --release --bin build-index -- resources/references.json.gz resources/references.ridx
```

The current runtime image does not use the generated `.ridx` file. It is kept for validation and future exact/IVF index strategies.
