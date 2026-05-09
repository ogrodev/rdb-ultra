use thiserror::Error;

use std::sync::Arc;

use crate::{
    classifier::{
        classify_payload_bytes_with_support, classify_reference_rule, classify_vector_with_support,
    },
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
    support_index: Option<Arc<dyn NearestNeighbors>>,
    normalization: Normalization,
    mcc_risk: MccRisk,
}

impl FraudEngine {
    pub fn new<I: NearestNeighbors + 'static>(
        index: I,
        normalization: Normalization,
        mcc_risk: MccRisk,
    ) -> Self {
        Self {
            support_index: Some(Arc::new(index)),
            normalization,
            mcc_risk,
        }
    }

    pub fn score_bytes(&self, body: &[u8]) -> Result<Decision, EngineError> {
        if let Some(decision) =
            classify_payload_bytes_with_support(body, self.support_index.as_deref(), &self.mcc_risk)
        {
            return Ok(decision);
        }
        let payload: FraudPayload = serde_json::from_slice(body)?;
        self.score_payload(&payload)
    }

    pub fn score_payload(&self, payload: &FraudPayload) -> Result<Decision, EngineError> {
        let vector = vectorize_payload(payload, &self.normalization, &self.mcc_risk)?;
        if self.support_index.is_some() {
            Ok(classify_vector_with_support(
                &vector,
                self.support_index.as_deref(),
            ))
        } else {
            Ok(classify_reference_rule(&vector))
        }
    }
}

impl FraudEngine {
    pub fn without_index(normalization: Normalization, mcc_risk: MccRisk) -> Self {
        Self {
            support_index: None,
            normalization,
            mcc_risk,
        }
    }
}
