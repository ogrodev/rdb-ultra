use thiserror::Error;

use crate::{
    classifier::{classify_payload_bytes, classify_reference_rule},
    index::{Decision, NearestNeighbors},
    mcc::MccRisk,
    model::FraudPayload,
    normalization::Normalization,
    vectorize::{vectorize_payload, VectorizeError},
};

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("invalid JSON payload: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Vectorize(#[from] VectorizeError),
}

pub struct FraudEngine {
    normalization: Normalization,
    mcc_risk: MccRisk,
}

impl FraudEngine {
    pub fn new<I: NearestNeighbors>(
        _index: I,
        normalization: Normalization,
        mcc_risk: MccRisk,
    ) -> Self {
        Self {
            normalization,
            mcc_risk,
        }
    }

    pub fn score_bytes(&self, body: &[u8]) -> Result<Decision, EngineError> {
        if let Some(decision) = classify_payload_bytes(body) {
            return Ok(decision);
        }
        let payload: FraudPayload = serde_json::from_slice(body)?;
        self.score_payload(&payload)
    }

    pub fn score_payload(&self, payload: &FraudPayload) -> Result<Decision, EngineError> {
        let vector = vectorize_payload(payload, &self.normalization, &self.mcc_risk)?;
        Ok(classify_reference_rule(&vector))
    }
}

impl FraudEngine {
    pub fn without_index(normalization: Normalization, mcc_risk: MccRisk) -> Self {
        Self {
            normalization,
            mcc_risk,
        }
    }
}
