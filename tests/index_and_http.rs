use std::sync::Arc;

use rinha_backend_v2::{
    engine::FraudEngine,
    http::handle_http_request,
    index::{NearestNeighbors, ReferenceIndex, ReferenceRecord},
    mcc::MccRisk,
    normalization::Normalization,
};

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
