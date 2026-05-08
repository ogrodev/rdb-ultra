use std::{fs::File, io::BufReader, path::Path};

use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Normalization {
    pub max_amount: f32,
    pub max_installments: f32,
    pub amount_vs_avg_ratio: f32,
    pub max_minutes: f32,
    pub max_km: f32,
    pub max_tx_count_24h: f32,
    pub max_merchant_avg_amount: f32,
}

impl Normalization {
    pub const fn standard() -> Self {
        Self {
            max_amount: 10_000.0,
            max_installments: 12.0,
            amount_vs_avg_ratio: 10.0,
            max_minutes: 1_440.0,
            max_km: 1_000.0,
            max_tx_count_24h: 20.0,
            max_merchant_avg_amount: 10_000.0,
        }
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let file = File::open(path)?;
        serde_json::from_reader(BufReader::new(file)).map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid normalization.json: {err}"),
            )
        })
    }
}
