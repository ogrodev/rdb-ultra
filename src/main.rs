use std::{env, sync::Arc};

use rinha_backend_v2::{
    binary_index::HourBucketIndex, engine::FraudEngine, http::serve, mcc::MccRisk,
    normalization::Normalization,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = env::var("API_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());

    let normalization = match env::var_os("NORMALIZATION_PATH") {
        Some(path) => Normalization::from_path(path)?,
        None => Normalization::standard(),
    };
    let mcc_risk = match env::var_os("MCC_RISK_PATH") {
        Some(path) => MccRisk::from_path(path)?,
        None => MccRisk::standard(),
    };

    let engine = match env::var_os("SUPPORT_INDEX_PATH") {
        Some(path) => Arc::new(FraudEngine::new(
            HourBucketIndex::open_dir(path)?,
            normalization,
            mcc_risk,
        )),
        None => Arc::new(FraudEngine::without_index(normalization, mcc_risk)),
    };

    serve(&addr, engine)?;
    Ok(())
}
