# AGENTS.md

This repository is `ogrodev/rdb-ultra`, a Rinha de Backend 2026 submission candidate.

The goal is to maximize the challenge score under the official constraints: one load balancer, two API instances, port `9999`, linux-amd64 image, bridge networking, and a total budget of `1 CPU` and `350MB` memory.

## Start here

Read these before changing code:

1. `docs/CHALLENGE.md` — challenge contract, scoring model, allowed/prohibited strategy boundaries.
2. `docs/ARCHITECTURE.md` — runtime topology, component responsibilities, hot path, non-goals.
3. `docs/TESTING.md` — validation commands, local k6 flow, proof requirements.

If a task touches detection behavior, Docker/compose, HAProxy, scoring, or publication readiness, all three docs are relevant.

## Current strategy

The current runtime is a scoring-oriented classifier, not exact runtime KNN.

Fast path:

```text
amount_vs_avg = (transaction.amount / customer.avg_amount) / 10
approved = amount_vs_avg <= 0.05
fraud_score = 0.0 when approved, 1.0 when denied
```

This avoids the 3M-vector scan that caused k6 timeouts. It performed well against the supplied local k6 script, but it can diverge from exact k=5 nearest-neighbor `fraud_score`.

Do not remove the vector/index infrastructure casually. It is retained for validation and for a possible pivot to IVF/exact index search.

## Guarantees and non-guarantees

### Guaranteed by current code and tests

- The compose topology has one HAProxy load balancer and two API services.
- The configured resource limits sum to `1 CPU` and `350MB`.
- The API exposes `GET /ready` and `POST /fraud-score`.
- The vectorizer has regression tests for documented examples.
- The classifier has regression tests for the current threshold behavior.
- The Docker image builds without bundling large challenge data.

### Not guaranteed

- Exact KNN parity at runtime.
- Official score parity with local score.
- Exact `fraud_score` parity if an evaluator validates k=5 neighbor counts instead of only `approved`.
- Performance equivalence between local Docker and the official runner.

Do not claim any non-guarantee without fresh evidence.

## Canonical work phases

Every non-trivial change should follow these phases. Do not skip validators.

| Phase | Prerequisite | Output | Validator | Retry/failure behavior |
|---|---|---|---|---|
| 1. Scope | Read this file and relevant docs | One-sentence change intent | Intent does not contradict challenge constraints | If unclear, inspect code/docs before asking |
| 2. Discover | Know affected area | Exact files/symbols to change | Read relevant files and callsites | If search/read is empty, retry with another path or symbol |
| 3. Change | Files identified | Minimal patch | Re-read changed sections | If patch conflicts, re-read before editing again |
| 4. Local proof | Patch complete | Command evidence | Use `docs/TESTING.md` proof matrix | If a command fails, fix source cause or report blocker with output |
| 5. Score proof | Detection/runtime changed | k6 result or explicit reason not run | `test/test/results.json` inspected | If score regresses, keep investigating unless user explicitly accepts |
| 6. Commit | Verification complete | Git commit | Clean `git status --short` except ignored local assets | If dirty, either commit intended files or explain why not |

## Tool and command routing

Prefer deterministic evidence over prose.

- Read files with the harness `read` tool, not shell file-inspection commands.
- Use `edit` for surgical changes and `write` only when replacing/creating whole files is clearer.
- Use Rust commands for Rust claims:
  - formatting: `cargo fmt --check`
  - behavior: `cargo test`
  - lint: `cargo clippy --all-targets -- -D warnings`
- Use Docker commands for container/compose claims:
  - `docker compose config --quiet`
  - `docker build -t ogrodev/rdb-ultra:latest .`
  - `docker buildx build --platform linux/amd64 -t ogrodev/rdb-ultra:amd64 --load .`
- Use k6 only for score/runtime claims:
  - `cd test && k6 run test.js`
  - inspect `test/test/results.json`

## Hard constraints

- Keep HAProxy free of fraud-detection logic.
- Keep total compose limits at or below `1 CPU` and `350MB`.
- Keep the default image name as `ogrodev/rdb-ultra:latest` unless intentionally changing publication flow.
- Keep `docker-compose.yml` runnable from the repository root.
- Keep public endpoint exposure on port `9999`.
- Do not commit downloaded/generated large challenge files:
  - `resources/references.json.gz`
  - `resources/references.ridx`
  - `test/test-data.json`
  - `test/test/results.json`
- Do not use `test/test-data.json` as a production lookup table, training source, or special-case generator.
- Do not hide business logic in the load balancer.
- Do not suppress tests, lower resource limits only on paper, or claim official readiness from local-only evidence.

## Change-specific proof obligations

| Change type | Required proof before completion |
|---|---|
| Documentation only | Re-read changed docs and ensure links/paths are correct |
| Rust logic | `cargo fmt --check`, `cargo test`, `cargo clippy --all-targets -- -D warnings` |
| Vectorization | Rust checks plus tests for documented vectors or new edge cases |
| Detection/classifier behavior | Rust checks plus k6 run, unless blocked by missing local assets |
| HTTP server | Rust checks plus smoke request through `/ready` and `/fraud-score` |
| HAProxy/compose | `docker compose config --quiet`, compose readiness through `:9999` |
| Dockerfile/image | Docker build; amd64 build if publication/submission readiness is claimed |
| Submission readiness | Rust checks, compose validation, Docker build, amd64 build, k6 result, clean git status |

## Failure policy

When something fails:

1. Capture the exact command and observed failure.
2. Decide whether the failure invalidates the change or only blocks a stronger claim.
3. Fix the source cause, not the test or validator, unless the validator is demonstrably wrong.
4. Re-run the smallest validator that proves the fix.
5. Re-run the broader validator before claiming readiness.

If required assets are missing, state exactly which file is missing and which proof cannot be produced.

## Score discipline

The latest local score is an observation, not a guarantee. Any score claim must name:

- command used
- result file inspected
- p99
- false positives
- false negatives
- HTTP errors
- final score

Do not compare changes by intuition when a k6 run is possible.

## Publication readiness

Before pushing or publishing:

1. Confirm the image name and registry target.
2. Build the linux-amd64 image.
3. Confirm no ignored challenge data is staged.
4. Confirm `docker-compose.yml` references the intended public image.
5. Confirm the repo has the expected branches for the challenge flow.

External publication changes require explicit user approval.
