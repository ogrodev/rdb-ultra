# AGENTS.md

This repository is `ogrodev/rdb-ultra`, a Rinha de Backend 2026 submission candidate.

We are trying to maximize the challenge score under the official constraints: one load balancer, two API instances, port `9999`, linux-amd64 image, bridge networking, and a total budget of `1 CPU` and `350MB` memory.

Start with these docs before changing code:

- `docs/CHALLENGE.md` — what the challenge asks for, scoring implications, and the main risk in our current strategy
- `docs/ARCHITECTURE.md` — current runtime topology, API server, HAProxy setup, classifier path, and retained vector/index code
- `docs/TESTING.md` — local verification commands, Docker/k6 flow, and latest observed local score

## Current strategy

The current runtime is a scoring-oriented classifier, not exact runtime KNN.

It extracts `transaction.amount` and `customer.avg_amount`, computes `amount_vs_avg`, and denies when:

```text
(amount / avg_amount) / 10 > 0.05
```

This avoids the 3M-vector scan that caused k6 timeouts. It is fast and matched the supplied local scoring script reasonably well, but it can diverge from exact k=5 nearest-neighbor fraud scores.

Do not remove the vector/index infrastructure casually. It is retained for validation and for a possible future pivot to IVF/exact index search.

## Working rules for agents

- Keep HAProxy free of fraud-detection logic.
- Keep total compose limits at or below `1 CPU` and `350MB`.
- Keep the default image name as `ogrodev/rdb-ultra:latest` unless intentionally changing publication flow.
- Do not commit downloaded large challenge files:
  - `resources/references.json.gz`
  - `resources/references.ridx`
  - `test/test-data.json`
  - `test/test/results.json`
- Do not use `test/test-data.json` as a lookup table or training source for production logic.
- If changing detection logic, run the Rust checks and a k6 validation before claiming improvement.

## Minimum verification before claiming readiness

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
docker compose config --quiet
docker build -t ogrodev/rdb-ultra:latest .
```

For score validation:

```sh
docker compose up -d
cd test
k6 run test.js
```

Then inspect:

```text
test/test/results.json
```
