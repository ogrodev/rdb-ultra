use rinha_backend_v2::{classifier::classify_payload_bytes, index::Decision};

#[test]
fn fast_payload_classifier_uses_transaction_amount_and_customer_average_only() {
    let body = br#"{
        "id":"tx-fast",
        "transaction":{"amount":100.0,"installments":1,"requested_at":"not-needed-on-fast-path"},
        "customer":{"avg_amount":100.0,"tx_count_24h":0,"known_merchants":[]},
        "merchant":{"id":"MERC-001","mcc":"5411","avg_amount":9999.0},
        "terminal":{"is_online":false,"card_present":true,"km_from_home":0.0},
        "last_transaction":null
    }"#;

    assert_eq!(
        classify_payload_bytes(body),
        Some(Decision::from_fraud_count(5))
    );
}

#[test]
fn fast_payload_classifier_approves_low_amount_vs_average() {
    let body = br#"{
        "transaction":{"amount":5.0,"installments":1,"requested_at":"not-needed-on-fast-path"},
        "customer":{"avg_amount":100.0,"tx_count_24h":0,"known_merchants":[]},
        "merchant":{"id":"MERC-001","mcc":"5411","avg_amount":9999.0},
        "terminal":{"is_online":false,"card_present":true,"km_from_home":0.0},
        "last_transaction":null
    }"#;

    assert_eq!(
        classify_payload_bytes(body),
        Some(Decision::from_fraud_count(0))
    );
}
