use std::{fs::File, path::Path, sync::Arc};

use rinha_backend_v2::{
    binary_index::MmapIndex,
    engine::FraudEngine,
    http::handle_http_request,
    index::{
        decide_from_slices, decide_pruned_by_dim2, quantize, NearestNeighbors, QuantizedVector,
        ReferenceIndex, ReferenceRecord, DIMS,
    },
    mcc::MccRisk,
    model::FraudPayload,
    normalization::Normalization,
    vectorize::vectorize_payload,
};
use serde::Deserialize;

fn vector(value: f32) -> [f32; 14] {
    [value; 14]
}

#[test]
fn index_uses_five_nearest_neighbors_and_denies_at_threshold() {
    let index = ReferenceIndex::from_records(vec![
        ReferenceRecord::new(vector(0.001), true),
        ReferenceRecord::new(vector(0.002), true),
        ReferenceRecord::new(vector(0.003), true),
        ReferenceRecord::new(vector(0.004), false),
        ReferenceRecord::new(vector(0.005), false),
        ReferenceRecord::new(vector(0.900), false),
    ]);

    let decision = index.decide(&vector(0.0));

    assert_eq!(decision.fraud_count, 3);
    assert_eq!(decision.fraud_score, 0.6);
    assert!(
        !decision.approved,
        "score 0.6 must be denied because approval requires score < 0.6"
    );
}

#[test]
fn index_approves_when_only_two_of_five_neighbors_are_fraud() {
    let index = ReferenceIndex::from_records(vec![
        ReferenceRecord::new(vector(0.001), true),
        ReferenceRecord::new(vector(0.002), true),
        ReferenceRecord::new(vector(0.003), false),
        ReferenceRecord::new(vector(0.004), false),
        ReferenceRecord::new(vector(0.005), false),
        ReferenceRecord::new(vector(0.900), true),
    ]);

    let decision = index.decide(&vector(0.0));

    assert_eq!(decision.fraud_count, 2);
    assert_eq!(decision.fraud_score, 0.4);
    assert!(decision.approved);
}

#[test]
fn http_ready_and_fraud_score_return_contract_json() {
    let index = Arc::new(ReferenceIndex::from_records(vec![
        ReferenceRecord::new(
            [
                0.0041, 0.1667, 0.05, 0.7826, 0.3333, -1.0, -1.0, 0.0292, 0.15, 0.0, 1.0, 0.0,
                0.15, 0.006,
            ],
            false,
        ),
        ReferenceRecord::new(
            [
                0.0042, 0.1667, 0.05, 0.7826, 0.3333, -1.0, -1.0, 0.0292, 0.15, 0.0, 1.0, 0.0,
                0.15, 0.006,
            ],
            false,
        ),
        ReferenceRecord::new(
            [
                0.0043, 0.1667, 0.05, 0.7826, 0.3333, -1.0, -1.0, 0.0292, 0.15, 0.0, 1.0, 0.0,
                0.15, 0.006,
            ],
            false,
        ),
        ReferenceRecord::new(
            [
                0.0044, 0.1667, 0.05, 0.7826, 0.3333, -1.0, -1.0, 0.0292, 0.15, 0.0, 1.0, 0.0,
                0.15, 0.006,
            ],
            false,
        ),
        ReferenceRecord::new(
            [
                0.0045, 0.1667, 0.05, 0.7826, 0.3333, -1.0, -1.0, 0.0292, 0.15, 0.0, 1.0, 0.0,
                0.15, 0.006,
            ],
            false,
        ),
    ]));
    let engine = FraudEngine::new(index, Normalization::standard(), MccRisk::standard());

    let ready = handle_http_request(b"GET /ready HTTP/1.1\r\nHost: localhost\r\n\r\n", &engine);
    let ready_text = String::from_utf8(ready).unwrap();
    assert!(ready_text.starts_with("HTTP/1.1 200 OK"), "{ready_text}");
    assert!(
        ready_text.contains("Connection: keep-alive"),
        "ready responses must keep the HAProxy backend connection reusable: {ready_text}"
    );

    let body = serde_json::json!({
        "id": "tx-1329056812",
        "transaction": { "amount": 41.12, "installments": 2, "requested_at": "2026-03-11T18:45:53Z" },
        "customer": { "avg_amount": 82.24, "tx_count_24h": 3, "known_merchants": ["MERC-003", "MERC-016"] },
        "merchant": { "id": "MERC-016", "mcc": "5411", "avg_amount": 60.25 },
        "terminal": { "is_online": false, "card_present": true, "km_from_home": 29.23 },
        "last_transaction": null
    })
    .to_string();
    let request = format!(
        "POST /fraud-score HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );

    let response = handle_http_request(request.as_bytes(), &engine);
    let text = String::from_utf8(response).unwrap();

    assert!(text.starts_with("HTTP/1.1 200 OK"), "{text}");
    assert!(
        text.ends_with("{\"approved\":true,\"fraud_score\":0.0}"),
        "{text}"
    );
}

#[test]
fn engine_uses_fast_reference_derived_rule_for_high_amount_vs_average() {
    let index = Arc::new(ReferenceIndex::from_records(vec![
        ReferenceRecord::new(vector(0.0), false),
        ReferenceRecord::new(vector(0.0), false),
        ReferenceRecord::new(vector(0.0), false),
        ReferenceRecord::new(vector(0.0), false),
        ReferenceRecord::new(vector(0.0), false),
    ]));
    let engine = FraudEngine::new(index, Normalization::standard(), MccRisk::standard());
    let payload = serde_json::json!({
        "id": "tx-rule",
        "transaction": { "amount": 100.0, "installments": 1, "requested_at": "2026-03-11T18:45:53Z" },
        "customer": { "avg_amount": 100.0, "tx_count_24h": 0, "known_merchants": ["MERC-016"] },
        "merchant": { "id": "MERC-016", "mcc": "5411", "avg_amount": 60.25 },
        "terminal": { "is_online": false, "card_present": true, "km_from_home": 0.0 },
        "last_transaction": null
    })
    .to_string();

    let decision = engine.score_bytes(payload.as_bytes()).unwrap();

    assert_eq!(decision.fraud_count, 5);
    assert!(!decision.approved);
}

#[test]
fn engine_uses_support_index_for_high_ratio_support_window() {
    let index = Arc::new(ReferenceIndex::from_records(vec![
        ReferenceRecord::new(vector(0.1), false),
        ReferenceRecord::new(vector(0.2), false),
        ReferenceRecord::new(vector(0.3), false),
        ReferenceRecord::new(vector(0.4), false),
        ReferenceRecord::new(vector(0.5), false),
    ]));
    let engine = FraudEngine::new(index, Normalization::standard(), MccRisk::standard());
    let payload = serde_json::json!({
        "id": "tx-support-window",
        "transaction": { "amount": 773.20, "installments": 3, "requested_at": "2026-03-22T18:47:28Z" },
        "customer": { "avg_amount": 472.65, "tx_count_24h": 4, "known_merchants": ["MERC-008", "MERC-018"] },
        "merchant": { "id": "MERC-008", "mcc": "5411", "avg_amount": 273.60 },
        "terminal": { "is_online": true, "card_present": false, "km_from_home": 76.21 },
        "last_transaction": { "timestamp": "2026-03-22T16:54:28Z", "km_from_current": 148.87 }
    })
    .to_string();

    let decision = engine.score_bytes(payload.as_bytes()).unwrap();

    assert_eq!(decision.fraud_count, 0);
    assert!(decision.approved);
}

#[test]
fn pruned_search_matches_full_scan_for_sorted_synthetic_queries() {
    let mut seed = 0x7269_6e68_612d_3236_u64;
    let mut vectors = Vec::with_capacity(4096);
    let mut labels = Vec::with_capacity(4096);

    for _ in 0..4096 {
        let mut vector = [0_i16; 16];
        for value in vector.iter_mut().take(DIMS) {
            *value = next_quantized(&mut seed);
        }
        vectors.push(vector);
        labels.push((next_u32(&mut seed) & 1) as u8);
    }
    sort_by_dim2(&mut vectors, &mut labels);

    for query_idx in 0..1000 {
        let mut query = [0_i16; 16];
        for value in query.iter_mut().take(DIMS) {
            *value = next_quantized(&mut seed);
        }

        let expected = decide_from_slices(&query, &vectors, &labels);
        let actual = decide_pruned_by_dim2(&query, &vectors, &labels);
        assert_eq!(actual, expected, "synthetic query {query_idx}");
    }
}

#[test]
fn pruned_search_matches_full_scan_when_tie_order_matters() {
    let query = quantized_with_dim2(1_000);
    let mut vectors = vec![
        vector_with_dims(800, 0),
        vector_with_dims(900, 174),
        vector_with_dims(1_100, 174),
        vector_with_dims(1_200, 0),
        vector_with_dims(1_300, 0),
        vector_with_dims(1_400, 0),
    ];
    let mut labels = vec![1, 0, 0, 0, 0, 0];
    sort_by_dim2(&mut vectors, &mut labels);

    let expected = decide_from_slices(&query, &vectors, &labels);
    let actual = decide_pruned_by_dim2(&query, &vectors, &labels);

    assert_eq!(actual, expected);
    assert_eq!(
        actual.fraud_count, 1,
        "lower sorted index must win equal-distance ties even when pruned traversal visits it later"
    );
}

#[test]
fn pruned_search_matches_full_scan_for_real_payload_samples_when_assets_present() {
    let data_path = Path::new("test/test-data.json");
    if !data_path.exists() {
        eprintln!(
            "skipping real payload parity check: {} is missing",
            data_path.display()
        );
        return;
    }

    let buckets = open_hour_buckets();
    for (hour, bucket) in buckets.iter().enumerate() {
        assert!(
            bucket.supports_soa(),
            "model/hour/support-h{hour:02}.idx must be regenerated as RINHIDX4 before running parity tests"
        );
    }

    let file = File::open(data_path).expect("test-data.json must be readable");
    let data: TestData = serde_json::from_reader(file).expect("test-data.json must be valid JSON");
    assert!(
        data.entries.len() >= 1000,
        "real payload parity check requires at least 1000 entries"
    );

    let normalization = Normalization::standard();
    let mcc_risk = MccRisk::standard();
    for (idx, entry) in data.entries.iter().take(1000).enumerate() {
        let vector = vectorize_payload(&entry.request, &normalization, &mcc_risk)
            .expect("real test payload must vectorize");
        let query = quantize(&vector);
        let bucket = &buckets[quantized_hour_bucket_for_test(query[3])];
        let expected = decide_from_slices(&query, bucket.vectors(), bucket.labels());
        let actual = bucket.decide_quantized(&query);
        assert_eq!(actual, expected, "real payload entry {idx}");
    }
}

#[derive(Deserialize)]
struct TestData {
    entries: Vec<TestEntry>,
}

#[derive(Deserialize)]
struct TestEntry {
    request: FraudPayload,
}

fn open_hour_buckets() -> Vec<MmapIndex> {
    (0..24)
        .map(|hour| {
            MmapIndex::open(format!("model/hour/support-h{hour:02}.idx"))
                .expect("hour bucket index must open")
        })
        .collect()
}

fn quantized_hour_bucket_for_test(hour: i16) -> usize {
    let clamped = i32::from(hour).clamp(0, 10_000);
    ((clamped * 23 + 5_000) / 10_000) as usize
}

fn sort_by_dim2(vectors: &mut [QuantizedVector], labels: &mut [u8]) {
    let mut entries = vectors
        .iter()
        .copied()
        .zip(labels.iter().copied())
        .collect::<Vec<_>>();
    entries.sort_by_key(|(vector, _)| vector[2]);

    for (idx, (vector, label)) in entries.into_iter().enumerate() {
        vectors[idx] = vector;
        labels[idx] = label;
    }
}

fn next_u32(seed: &mut u64) -> u32 {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    (*seed >> 32) as u32
}

fn next_quantized(seed: &mut u64) -> i16 {
    (next_u32(seed) % 20_001) as i16 - 10_000
}

fn quantized_with_dim2(dim2: i16) -> QuantizedVector {
    let mut vector = [0_i16; 16];
    vector[2] = dim2;
    vector
}

fn vector_with_dims(dim2: i16, dim0: i16) -> QuantizedVector {
    let mut vector = [0_i16; 16];
    vector[0] = dim0;
    vector[2] = dim2;
    vector
}
