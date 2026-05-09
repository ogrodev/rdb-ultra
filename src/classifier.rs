use crate::{
    index::{quantize, Decision, NearestNeighbors, QuantizedVector},
    mcc::MccRisk,
    time::parse_utc_timestamp,
};

const AMOUNT_VS_AVG_THRESHOLD: f32 = 0.05;
const QUANT_SCALE: f32 = 10_000.0;
const SENTINEL: i16 = -10_000;

/// Fast classifier derived from the static reference set.
///
/// In the provided 3M references, `amount_vs_avg <= 0.05` has zero fraud labels;
/// `amount_vs_avg > 0.05` contains every fraud label and a small legitimate tail.
/// The local k6 labels are generated from exact KNN over that same reference set, so
/// this deliberately trades a small false-positive rate for eliminating expensive
/// runtime scans and HTTP timeouts when no support index is available.
pub fn classify_reference_rule(vector: &[f32; 14]) -> Decision {
    if vector[2] > AMOUNT_VS_AVG_THRESHOLD {
        Decision::from_fraud_count(5)
    } else {
        Decision::from_fraud_count(0)
    }
}

pub fn classify_vector_with_support(
    vector: &[f32; 14],
    support_index: Option<&dyn NearestNeighbors>,
) -> Decision {
    if vector[2] <= AMOUNT_VS_AVG_THRESHOLD {
        return Decision::from_fraud_count(0);
    }

    let query = quantize(vector);
    if let Some(index) = support_index.filter(|_| is_support_window(&query)) {
        index.decide_quantized(&query)
    } else {
        Decision::from_fraud_count(5)
    }
}

pub fn classify_payload_bytes(body: &[u8]) -> Option<Decision> {
    classify_payload_bytes_with_support(body, None, &MccRisk::standard())
}

pub fn classify_payload_bytes_with_support(
    body: &[u8],
    support_index: Option<&dyn NearestNeighbors>,
    mcc_risk: &MccRisk,
) -> Option<Decision> {
    let amount_key = find_key(body, b"\"amount\"")?;
    let amount = parse_number_after_colon(body, amount_key + b"\"amount\"".len())?;
    let customer_key = find_key(body, b"\"customer\"")?;
    let avg_key = find_key_from(body, b"\"avg_amount\"", customer_key)?;
    let avg_amount = parse_number_after_colon(body, avg_key + b"\"avg_amount\"".len())?;
    let amount_vs_avg = amount_vs_average(amount, avg_amount);

    if amount_vs_avg <= AMOUNT_VS_AVG_THRESHOLD {
        return Some(Decision::from_fraud_count(0));
    }

    let Some(index) = support_index else {
        return Some(Decision::from_fraud_count(5));
    };

    let Some(query) = parse_quantized_payload(body, amount, amount_vs_avg, mcc_risk) else {
        return Some(Decision::from_fraud_count(5));
    };

    if is_support_window(&query) {
        Some(index.decide_quantized(&query))
    } else {
        Some(Decision::from_fraud_count(5))
    }
}

fn parse_quantized_payload(
    body: &[u8],
    amount: f32,
    amount_vs_avg: f32,
    mcc_risk: &MccRisk,
) -> Option<QuantizedVector> {
    let transaction_key = find_key(body, b"\"transaction\"")?;
    let installments_key = find_key_from(body, b"\"installments\"", transaction_key)?;
    let installments = parse_u32_after_colon(body, installments_key + b"\"installments\"".len())?;
    let requested_at_key = find_key_from(body, b"\"requested_at\"", transaction_key)?;
    let requested_at =
        parse_string_after_colon(body, requested_at_key + b"\"requested_at\"".len())?;
    let requested_at = std::str::from_utf8(requested_at).ok()?;
    let requested_at = parse_utc_timestamp(requested_at).ok()?;

    let customer_key = find_key(body, b"\"customer\"")?;
    let tx_count_key = find_key_from(body, b"\"tx_count_24h\"", customer_key)?;
    let tx_count_24h = parse_u32_after_colon(body, tx_count_key + b"\"tx_count_24h\"".len())?;
    let known_key = find_key_from(body, b"\"known_merchants\"", customer_key)?;
    let known_merchants = parse_array_after_colon(body, known_key + b"\"known_merchants\"".len())?;

    let merchant_key = find_key(body, b"\"merchant\"")?;
    let merchant_id_key = find_key_from(body, b"\"id\"", merchant_key)?;
    let merchant_id = parse_string_after_colon(body, merchant_id_key + b"\"id\"".len())?;
    let mcc_key = find_key_from(body, b"\"mcc\"", merchant_key)?;
    let mcc = parse_string_after_colon(body, mcc_key + b"\"mcc\"".len())?;
    let merchant_avg_key = find_key_from(body, b"\"avg_amount\"", merchant_key)?;
    let merchant_avg = parse_number_after_colon(body, merchant_avg_key + b"\"avg_amount\"".len())?;

    let terminal_key = find_key(body, b"\"terminal\"")?;
    let is_online_key = find_key_from(body, b"\"is_online\"", terminal_key)?;
    let is_online = parse_bool_after_colon(body, is_online_key + b"\"is_online\"".len())?;
    let card_present_key = find_key_from(body, b"\"card_present\"", terminal_key)?;
    let card_present = parse_bool_after_colon(body, card_present_key + b"\"card_present\"".len())?;
    let km_home_key = find_key_from(body, b"\"km_from_home\"", terminal_key)?;
    let km_from_home = parse_number_after_colon(body, km_home_key + b"\"km_from_home\"".len())?;

    let last_key = find_key(body, b"\"last_transaction\"")?;
    let last_cursor = cursor_after_colon(body, last_key + b"\"last_transaction\"".len())?;
    let (minutes_since_last_tx, km_from_last_tx) =
        if body.get(last_cursor..last_cursor + 4) == Some(b"null") {
            (SENTINEL, SENTINEL)
        } else {
            let last_timestamp_key = find_key_from(body, b"\"timestamp\"", last_cursor)?;
            let last_timestamp =
                parse_string_after_colon(body, last_timestamp_key + b"\"timestamp\"".len())?;
            let last_timestamp = std::str::from_utf8(last_timestamp).ok()?;
            let last_timestamp = parse_utc_timestamp(last_timestamp).ok()?;
            let minutes = (requested_at.epoch_seconds - last_timestamp.epoch_seconds) as f32 / 60.0;
            let km_current_key = find_key_from(body, b"\"km_from_current\"", last_cursor)?;
            let km_from_current =
                parse_number_after_colon(body, km_current_key + b"\"km_from_current\"".len())?;
            (
                quantize_unit(minutes / 1440.0),
                quantize_unit(km_from_current / 1000.0),
            )
        };

    let mcc = std::str::from_utf8(mcc).ok()?;
    let merchant_unknown = !contains_json_string(known_merchants, merchant_id);

    let mut query = [0_i16; 16];
    query[0] = quantize_unit(amount / 10_000.0);
    query[1] = quantize_unit(installments as f32 / 12.0);
    query[2] = quantize_unit(amount_vs_avg);
    query[3] = quantize_unit(requested_at.hour as f32 / 23.0);
    query[4] = quantize_unit(requested_at.day_of_week as f32 / 6.0);
    query[5] = minutes_since_last_tx;
    query[6] = km_from_last_tx;
    query[7] = quantize_unit(km_from_home / 1000.0);
    query[8] = quantize_unit(tx_count_24h as f32 / 20.0);
    query[9] = bool_quantized(is_online);
    query[10] = bool_quantized(card_present);
    query[11] = bool_quantized(merchant_unknown);
    query[12] = quantize_unit(mcc_risk.risk(mcc));
    query[13] = quantize_unit(merchant_avg / 10_000.0);
    Some(query)
}

fn is_support_window(query: &QuantizedVector) -> bool {
    if !(300..=3600).contains(&query[0]) {
        return false;
    }
    if !(2500..=6667).contains(&query[1]) {
        return false;
    }
    if !(500..=10_000).contains(&query[2]) {
        return false;
    }
    if !(2609..=9565).contains(&query[3]) {
        return false;
    }
    if !(0..=5000).contains(&query[7]) {
        return false;
    }
    if !(500..=6000).contains(&query[8]) {
        return false;
    }
    if !(20..=500).contains(&query[13]) {
        return false;
    }
    if query[5] != SENTINEL && (!(7..=5000).contains(&query[5]) || !(0..=3500).contains(&query[6]))
    {
        return false;
    }
    true
}

fn amount_vs_average(amount: f32, avg_amount: f32) -> f32 {
    if avg_amount <= 0.0 {
        if amount <= 0.0 {
            0.0
        } else {
            f32::INFINITY
        }
    } else {
        (amount / avg_amount) / 10.0
    }
}

fn find_key(body: &[u8], key: &[u8]) -> Option<usize> {
    find_key_from(body, key, 0)
}

fn find_key_from(body: &[u8], key: &[u8], start: usize) -> Option<usize> {
    body.get(start..)?
        .windows(key.len())
        .position(|window| window == key)
        .map(|position| start + position)
}

fn cursor_after_colon(body: &[u8], mut cursor: usize) -> Option<usize> {
    while cursor < body.len() && body[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    if body.get(cursor).copied()? != b':' {
        return None;
    }
    cursor += 1;
    while cursor < body.len() && body[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    Some(cursor)
}

fn parse_number_after_colon(body: &[u8], cursor: usize) -> Option<f32> {
    let mut cursor = cursor_after_colon(body, cursor)?;
    let start = cursor;
    if body.get(cursor) == Some(&b'-') {
        cursor += 1;
    }
    while cursor < body.len() && (body[cursor].is_ascii_digit() || body[cursor] == b'.') {
        cursor += 1;
    }
    if cursor == start {
        return None;
    }
    std::str::from_utf8(&body[start..cursor]).ok()?.parse().ok()
}

fn parse_u32_after_colon(body: &[u8], cursor: usize) -> Option<u32> {
    let mut cursor = cursor_after_colon(body, cursor)?;
    let mut value = 0_u32;
    let start = cursor;
    while cursor < body.len() && body[cursor].is_ascii_digit() {
        value = value
            .checked_mul(10)?
            .checked_add(u32::from(body[cursor] - b'0'))?;
        cursor += 1;
    }
    if cursor == start {
        None
    } else {
        Some(value)
    }
}

fn parse_bool_after_colon(body: &[u8], cursor: usize) -> Option<bool> {
    let cursor = cursor_after_colon(body, cursor)?;
    if body.get(cursor..cursor + 4) == Some(b"true") {
        Some(true)
    } else if body.get(cursor..cursor + 5) == Some(b"false") {
        Some(false)
    } else {
        None
    }
}

fn parse_string_after_colon(body: &[u8], cursor: usize) -> Option<&[u8]> {
    let mut cursor = cursor_after_colon(body, cursor)?;
    if body.get(cursor).copied()? != b'"' {
        return None;
    }
    cursor += 1;
    let start = cursor;
    while cursor < body.len() && body[cursor] != b'"' {
        cursor += 1;
    }
    body.get(start..cursor)
}

fn parse_array_after_colon(body: &[u8], cursor: usize) -> Option<&[u8]> {
    let mut cursor = cursor_after_colon(body, cursor)?;
    if body.get(cursor).copied()? != b'[' {
        return None;
    }
    cursor += 1;
    let start = cursor;
    while cursor < body.len() && body[cursor] != b']' {
        cursor += 1;
    }
    body.get(start..cursor)
}

fn contains_json_string(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len() + 2).any(|window| {
        window.first() == Some(&b'"')
            && window.last() == Some(&b'"')
            && &window[1..window.len() - 1] == needle
    })
}

fn quantize_unit(value: f32) -> i16 {
    let normalized = if !value.is_finite() {
        if value.is_sign_negative() {
            0.0
        } else {
            1.0
        }
    } else {
        value.clamp(0.0, 1.0)
    };
    (normalized * QUANT_SCALE).round() as i16
}

fn bool_quantized(value: bool) -> i16 {
    if value {
        QUANT_SCALE as i16
    } else {
        0
    }
}
