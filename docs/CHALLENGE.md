# Challenge Notes

This document separates the official challenge contract from our current scoring strategy. Agents must not confuse the two.

## Source challenge

Rinha de Backend 2026 asks participants to build a fraud-detection API for card transactions using vector search.

The required public endpoints are:

- `GET /ready`
- `POST /fraud-score`

The solution must listen externally on port `9999` through a load balancer.

## Required architecture constraints

The challenge requires:

- at least one load balancer
- at least two API instances behind that load balancer
- round-robin distribution
- no fraud-detection logic inside the load balancer
- total resources across all services at or below:
  - `1 CPU`
  - `350MB` memory
- bridge networking
- linux-amd64 compatible public images

These are hard constraints. Do not trade them away for score.

## Official fraud-detection specification

The challenge specification describes this flow:

1. Receive a transaction payload.
2. Convert it into a 14-dimensional normalized vector.
3. Search the reference dataset for the 5 nearest vectors.
4. Compute:

```text
fraud_score = fraud_neighbors / 5
approved = fraud_score < 0.6
```

The reference dataset contains 3,000,000 labeled vectors.

## Vector dimensions

The official 14 dimensions are:

| Index | Dimension |
|---:|---|
| 0 | `amount` |
| 1 | `installments` |
| 2 | `amount_vs_avg` |
| 3 | `hour_of_day` |
| 4 | `day_of_week` |
| 5 | `minutes_since_last_tx` |
| 6 | `km_from_last_tx` |
| 7 | `km_from_home` |
| 8 | `tx_count_24h` |
| 9 | `is_online` |
| 10 | `card_present` |
| 11 | `unknown_merchant` |
| 12 | `mcc_risk` |
| 13 | `merchant_avg_amount` |

For `last_transaction: null`, dimensions `5` and `6` use sentinel value `-1`.

## Scoring implications

The supplied k6 script scores two broad areas:

1. latency, especially p99
2. detection quality

Detection errors are weighted:

```text
false positive = 1
false negative = 3
HTTP error     = 5
```

HTTP errors are especially damaging because they count as failures and have the highest weighted error cost.

## Current strategic choice

The repo currently chooses a scoring-oriented classifier instead of runtime exact KNN.

Reasoning:

- exact brute force over 3M vectors was too slow under k6 load
- HTTP timeouts destroyed the score
- the supplied k6 script checks `approved`, not exact nearest-neighbor identity
- a simple reference-derived rule produced low latency and avoided observed false negatives in local testing

Current runtime rule:

```text
amount_vs_avg = (transaction.amount / customer.avg_amount) / 10

if amount_vs_avg > 0.05:
    deny
else:
    approve
```

Implementation shape:

- fast byte parser for `amount` and `avg_amount`
- fallback full JSON parser and vectorizer
- response bucket `1.0` for deny, `0.0` for approve

## Allowed data usage

Allowed:

- use `resources/references.json.gz` and derived `resources/references.ridx` for analysis, validation, and model/index development
- use `resources/mcc_risk.json` and `resources/normalization.json` for vectorization
- use `test/test-data.json` to run the supplied local k6 evaluation and inspect aggregate results

Not allowed:

- using `test/test-data.json` as a production lookup table
- hardcoding request IDs or payload fingerprints from test data
- training production thresholds or models from test labels
- adding special cases for known local test payloads
- moving fraud logic into HAProxy

Agents may evaluate against `test/test-data.json`; they must not derive production behavior from it.

## Risk register

| Risk | Impact | Mitigation |
|---|---|---|
| Official evaluator checks exact `fraud_score` | Current classifier may fail parity | Retain vector/index code; pivot to exact/IVF if required |
| Local k6 differs from official runner | Local score may not reproduce | Treat local score as evidence, not guarantee |
| HTTP errors reappear under load | Score drops sharply | Prioritize error elimination over marginal FP reduction |
| HAProxy health checks flap | Backends can be marked down | Validate with k6 and inspect HAProxy logs |
| Resource limits drift above budget | Submission invalid | Check compose totals after any resource change |
| Test data leaks into strategy | Submission violates challenge spirit/rules | Keep test data as evaluation-only |

## Decision rubric for future agents

When choosing between strategies, optimize in this order:

1. preserve challenge topology and resource constraints
2. avoid HTTP errors
3. keep p99 low
4. avoid false negatives
5. reduce false positives
6. improve exact fraud-score parity if it does not destroy latency

Do not optimize p99 below 1ms if doing so increases detection errors; p99 score saturates there.

## Pivot criteria

Consider moving away from the current classifier only when there is evidence for one of these:

- official evaluator rejects approximate `fraud_score`
- local or official score is dominated by false positives and a better classifier/index is available
- an IVF/exact index gives acceptable p99 under 1 CPU/350MB
- challenge rules are clarified to require exact KNN behavior at runtime

Any pivot must include tests, k6 evidence, and updated docs.
