use rinha_backend_v2::{
    mcc::MccRisk, model::FraudPayload, normalization::Normalization, vectorize::vectorize_payload,
};

fn assert_close(actual: &[f32; 14], expected: [f32; 14]) {
    for (idx, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
        assert!(
            (a - e).abs() <= 0.00015,
            "dimension {idx}: expected {e}, got {a}; vector={actual:?}"
        );
    }
}

#[test]
fn vectorizes_documented_legitimate_transaction() {
    let payload: FraudPayload = serde_json::from_value(serde_json::json!({
        "id": "tx-1329056812",
        "transaction": { "amount": 41.12, "installments": 2, "requested_at": "2026-03-11T18:45:53Z" },
        "customer": { "avg_amount": 82.24, "tx_count_24h": 3, "known_merchants": ["MERC-003", "MERC-016"] },
        "merchant": { "id": "MERC-016", "mcc": "5411", "avg_amount": 60.25 },
        "terminal": { "is_online": false, "card_present": true, "km_from_home": 29.23 },
        "last_transaction": null
    }))
    .unwrap();

    let vector =
        vectorize_payload(&payload, &Normalization::standard(), &MccRisk::standard()).unwrap();

    assert_close(
        &vector,
        [
            0.0041, 0.1667, 0.05, 0.7826, 0.3333, -1.0, -1.0, 0.0292, 0.15, 0.0, 1.0, 0.0, 0.15,
            0.006,
        ],
    );
}

#[test]
fn vectorizes_documented_fraudulent_transaction_with_clamping() {
    let payload: FraudPayload = serde_json::from_value(serde_json::json!({
        "id": "tx-3330991687",
        "transaction": { "amount": 9505.97, "installments": 10, "requested_at": "2026-03-14T05:15:12Z" },
        "customer": { "avg_amount": 81.28, "tx_count_24h": 20, "known_merchants": ["MERC-008", "MERC-007", "MERC-005"] },
        "merchant": { "id": "MERC-068", "mcc": "7802", "avg_amount": 54.86 },
        "terminal": { "is_online": false, "card_present": true, "km_from_home": 952.27 },
        "last_transaction": null
    }))
    .unwrap();

    let vector =
        vectorize_payload(&payload, &Normalization::standard(), &MccRisk::standard()).unwrap();

    assert_close(
        &vector,
        [
            0.9506, 0.8333, 1.0, 0.2174, 0.8333, -1.0, -1.0, 0.9523, 1.0, 0.0, 1.0, 1.0, 0.75,
            0.0055,
        ],
    );
}

#[test]
fn vectorizes_previous_transaction_distance_and_minutes() {
    let payload: FraudPayload = serde_json::from_value(serde_json::json!({
        "id": "tx-3576980410",
        "transaction": { "amount": 384.88, "installments": 3, "requested_at": "2026-03-11T20:23:35Z" },
        "customer": { "avg_amount": 769.76, "tx_count_24h": 3, "known_merchants": ["MERC-009", "MERC-001", "MERC-001"] },
        "merchant": { "id": "MERC-001", "mcc": "5912", "avg_amount": 298.95 },
        "terminal": { "is_online": false, "card_present": true, "km_from_home": 13.7090520965 },
        "last_transaction": { "timestamp": "2026-03-11T14:58:35Z", "km_from_current": 18.8626479774 }
    }))
    .unwrap();

    let vector =
        vectorize_payload(&payload, &Normalization::standard(), &MccRisk::standard()).unwrap();

    assert!(
        (vector[5] - (325.0 / 1440.0)).abs() <= 0.00001,
        "minutes dimension wrong: {}",
        vector[5]
    );
    assert!(
        (vector[6] - 0.018862648).abs() <= 0.00001,
        "km from last wrong: {}",
        vector[6]
    );
    assert_eq!(
        vector[11], 0.0,
        "merchant should be known even when repeated in known_merchants"
    );
}
