use thiserror::Error;

use crate::{
    mcc::MccRisk,
    model::FraudPayload,
    normalization::Normalization,
    time::{parse_utc_timestamp, TimestampError},
};

#[derive(Debug, Error)]
pub enum VectorizeError {
    #[error(transparent)]
    Timestamp(#[from] TimestampError),
}

pub fn vectorize_payload(
    payload: &FraudPayload,
    normalization: &Normalization,
    mcc_risk: &MccRisk,
) -> Result<[f32; 14], VectorizeError> {
    let requested_at = parse_utc_timestamp(&payload.transaction.requested_at)?;

    let amount = clamp(payload.transaction.amount / normalization.max_amount);
    let installments =
        clamp(payload.transaction.installments as f32 / normalization.max_installments);
    let amount_vs_avg = clamp(safe_amount_ratio(
        payload.transaction.amount,
        payload.customer.avg_amount,
        normalization.amount_vs_avg_ratio,
    ));
    let hour_of_day = requested_at.hour as f32 / 23.0;
    let day_of_week = requested_at.day_of_week as f32 / 6.0;

    let (minutes_since_last_tx, km_from_last_tx) = match &payload.last_transaction {
        Some(last) => {
            let previous = parse_utc_timestamp(&last.timestamp)?;
            let minutes = (requested_at.epoch_seconds - previous.epoch_seconds) as f32 / 60.0;
            (
                clamp(minutes / normalization.max_minutes),
                clamp(last.km_from_current / normalization.max_km),
            )
        }
        None => (-1.0, -1.0),
    };

    let km_from_home = clamp(payload.terminal.km_from_home / normalization.max_km);
    let tx_count_24h = clamp(payload.customer.tx_count_24h as f32 / normalization.max_tx_count_24h);
    let is_online = f32::from(payload.terminal.is_online);
    let card_present = f32::from(payload.terminal.card_present);
    let unknown_merchant = f32::from(
        !payload
            .customer
            .known_merchants
            .iter()
            .any(|known| known == &payload.merchant.id),
    );
    let mcc_risk = mcc_risk.risk(&payload.merchant.mcc);
    let merchant_avg_amount =
        clamp(payload.merchant.avg_amount / normalization.max_merchant_avg_amount);

    Ok([
        amount,
        installments,
        amount_vs_avg,
        hour_of_day,
        day_of_week,
        minutes_since_last_tx,
        km_from_last_tx,
        km_from_home,
        tx_count_24h,
        is_online,
        card_present,
        unknown_merchant,
        mcc_risk,
        merchant_avg_amount,
    ])
}

fn safe_amount_ratio(amount: f32, average: f32, amount_vs_avg_ratio: f32) -> f32 {
    if average <= 0.0 || amount_vs_avg_ratio <= 0.0 {
        if amount <= 0.0 {
            0.0
        } else {
            f32::INFINITY
        }
    } else {
        (amount / average) / amount_vs_avg_ratio
    }
}

pub fn clamp(value: f32) -> f32 {
    if !value.is_finite() {
        if value.is_sign_negative() {
            0.0
        } else {
            1.0
        }
    } else {
        value.clamp(0.0, 1.0)
    }
}
