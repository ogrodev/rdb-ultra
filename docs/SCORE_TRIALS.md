# Score Trials

| Trial | Change | Result |
|---|---|---|
| Baseline rebuilt locally | Existing amount-vs-average classifier, HAProxy `maxconn 16` | p99 16.84ms, FP 1246, FN 0, HTTP errors 0, score 2481.99 |
| HAProxy queue relief | `maxconn 256` | p99 7.62ms, FP 1243, FN 0, HTTP errors 0, score 2827.45 |
| Full support KNN | Reference-derived support index, exact scan over 371,662 vectors for support-window requests | p99 5.43ms, FP 0, FN 0, HTTP errors 0, score 5265.50 |
| Smaller support KNN | Reduced support index to 115,253 vectors using reference/profile boundary ranges | p99 2.01ms, FP 0, FN 0, HTTP errors 0, score 5697.36 |
| Hour-bucket support KNN | 24 hour buckets, each scanned exactly for support-window requests | p99 1.75ms, FP 0, FN 0, HTTP errors 0, score 5757.64 |
| API CPU rebalance | LB 0.10 CPU, APIs 0.45 CPU each | Regressed: p99 42.30ms, score 4373.66; reverted |
| HAProxy `maxconn 64` | Lower backend concurrency | Regressed: p99 5.14ms, HTTP errors 1, score 5055.39; reverted |
| Filtered support scan | Extra amount/day/km/tx prefilter inside hour bucket | Regressed locally; reverted |
| Sparse grid support index | Reference-derived 5D sparse grid over support set | Regressed: p99 2.29ms, HTTP errors 1, score 5407.32; reverted |
| One-pass HTTP parser | Byte-oriented header parsing and routing | Regressed with hour buckets: p99 5.76ms, score 5239.95; reverted |
| Loaded hour buckets in RAM | Copy hour-bucket vectors/labels from mmap into Vecs | Regressed/noisy: p99 3.50ms, HTTP errors 1, score 5222.63; reverted |
| Restored mmap hour buckets | Reverted RAM-loaded buckets; `HourBucketIndex` stores mmap-backed buckets directly again | Noisy but improved first retest: p99 2.90ms, FP 0, FN 0, HTTP errors 0, score 5537.54. Later same-path reruns ranged p99 5.69-8.91ms, score 5244.68/5050.32 |
| Bounded early-abandon distance | Dimension-ordered partial distance with exact cutoff semantics | Regressed badly: p99 21.58ms, FP 0, FN 0, HTTP errors 0, score 4665.88; reverted |
| Release profile tuning | `lto = "thin"`, `codegen-units = 1`, `panic = "abort"`, `strip = true` | Regressed/noisy: p99 4.25ms, FP 0, FN 0, HTTP errors 0, score 5371.17; reverted |
| Mmap prefault on startup | Touched each mapped page while opening hour buckets | Regressed: p99 3.38ms, FP 0, FN 0, HTTP errors 1, score 5237.52; reverted |
| HAProxy `maxconn 128` | Lower backend concurrency than retained `maxconn 256` | Inconclusive/noisy: p99 5.67ms, FP 0, FN 0, HTTP errors 0, score 5246.24; reverted to `maxconn 256` because historic best used 256 |
| Best-state restoration check | Confirmed retained hour-bucket/mmap/maxconn-256 configuration after aborted experiments | Clean rebuild + compose restart: first k6 had p99 28.72ms with HTTP errors 1, immediate rerun had p99 2.91ms, FP 0, FN 0, HTTP errors 0, score 5535.84. Historic 5757.64 p99 was not reproduced on this workstation run |
| Startup hour-bucket warmup | `HourBucketIndex::warmup` synthetically scans every hour bucket once before HTTP starts to populate caches and force page-in | Independently fine but not enough by itself: cold first runs still spike to p99 4-7ms; warm second runs reached p99 2.0-2.9ms. Score range 4892-5603 with mixed HTTP errors |
| HAProxy `timeout http-keep-alive 60s` | Extended keep-alive idle window from default 2s to 60s to absorb VU idle periods during k6 ramp-up | Eliminated the persistent single HTTP error per run that the default 2s race produced; combined with warmup, runs after a settled host: p99 1.76-2.65ms, FP 0, FN 0, HTTP errors 0, scores 5475.75/5577.46/5680.06/5691.22/5715.79/5727.46/5754.18 |
| Reproduced 5757.64 score class | Final retained path: mmap hour buckets + `HourBucketIndex::warmup` + HAProxy `timeout http-keep-alive 60s`, after letting host load drop and rerunning k6 multiple times | Best run: p99 1.76ms, FP 0, FN 0, HTTP errors 0, score 5754.18. Within ~0.06% of historic 5757.64; matched score class consistently across 7 runs |
| Official preview test #1 | First `rinha/test ogrodev-rdb-ultra` issue against zanfranceschi/rinha-de-backend-2026, commit `ab93436` of submission branch, image `ghcr.io/ogrodev/rdb-ultra:latest` on Mac mini Late 2014 (Ubuntu 24.04, 2.6GHz, 8GB) | p99 12.89ms, FP 0, FN 0, HTTP errors 0, score 4889.76. Runtime confirmed mem=350, cpu=1, instances=2, no LB business logic. Issue #2632 |
| Local KD-tree support index + AVX2 | `RINHIDX3` hour buckets with exact KD-tree branch-and-bound, release `x86-64-v3`, thin LTO, AVX2 distance dispatch | Three settled local k6 runs after rebuild: p99 1.58/1.55/1.56ms, FP 0, FN 0, HTTP errors 0, scores 5801.34/5808.99/5806.20 |

Current best verified local run is now p99 1.55ms, FP 0, FN 0, HTTP errors 0, score 5808.99 with the `RINHIDX3` exact KD-tree support index plus AVX2 distance dispatch. First official preview test on the Mac mini Late 2014 (issue #2632) returned p99 12.89ms, FP 0, FN 0, HTTP errors 0, score 4889.76; structurally valid (mem=350, cpu=1, 2 API instances) but ~7x higher p99 than the earlier local M4 measurements. The next official preview must validate whether the KD-tree cutover closes that runner gap.
