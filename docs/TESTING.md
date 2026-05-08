# Testing

## Required local assets

The challenge repository provides the large assets used for local validation:

- `resources/references.json.gz`
- `test/test-data.json`
- `test/test.js`
- `test/smoke.js`

In this repo, large generated/downloaded artifacts are intentionally ignored by git:

- `resources/references.json.gz`
- `resources/references.ridx`
- `test/test-data.json`
- `test/test/results.json`

The small challenge config files are tracked:

- `resources/mcc_risk.json`
- `resources/normalization.json`
- `resources/example-payloads.json`

## Rust tests

Run unit/integration tests:

```sh
cargo test
```

Run clippy with warnings as errors:

```sh
cargo clippy --all-targets -- -D warnings
```

Format check:

```sh
cargo fmt --check
```

A complete local code check is:

```sh
cargo fmt --check && cargo test && cargo clippy --all-targets -- -D warnings
```

## Docker build

Build the local image:

```sh
docker build -t ogrodev/rdb-ultra:latest .
```

Build an amd64 image locally from Apple Silicon:

```sh
docker buildx build --platform linux/amd64 -t ogrodev/rdb-ultra:amd64 --load .
```

The official submission image must be publicly available and linux-amd64 compatible.

## Compose validation

Validate compose syntax:

```sh
docker compose config --quiet
```

Start the stack:

```sh
docker compose up -d
```

Check readiness through the load balancer:

```sh
curl -i http://localhost:9999/ready
```

Stop the stack:

```sh
docker compose down
```

## k6 challenge run

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

Important fields:

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

Treat this as an observed local run, not a guaranteed official score. Local Docker, CPU scheduling, and official runner conditions may differ.

## Index-building tools

The repo retains a binary index builder for experimentation:

```sh
cargo run --release --bin build-index -- resources/references.json.gz resources/references.ridx
```

The current runtime image does not use the generated `.ridx` file. It is kept for validation and future exact/IVF index strategies.
