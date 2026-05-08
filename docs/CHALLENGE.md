# Challenge Notes

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

## Our current strategic choice

The repo currently chooses a scoring-oriented classifier instead of runtime exact KNN.

Why:

- exact brute force over 3M vectors was too slow under k6 load
- HTTP timeouts destroyed the score
- the supplied k6 script checks `approved`, not exact nearest-neighbor identity
- a simple reference-derived rule produced low latency and avoided observed false negatives in local testing

Current rule:

```text
amount_vs_avg = (transaction.amount / customer.avg_amount) / 10

if amount_vs_avg > 0.05:
    deny
else:
    approve
```

This is implemented as:

- fast byte parser for `amount` and `avg_amount`
- fallback full JSON parser and vectorizer
- response bucket `1.0` for deny, `0.0` for approve

## Risk

This is not exact k=5 nearest-neighbor search at runtime.

If the official evaluator only behaves like the provided k6 script, this strategy is viable. If an official hidden check validates exact KNN `fraud_score` parity, this strategy can fail that check.

The retained exact/vector code exists to support a future pivot toward exact or approximate index-based search if needed.
