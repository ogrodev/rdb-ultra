use std::{collections::HashMap, fs::File, io::BufReader, path::Path};

#[derive(Debug, Clone)]
pub struct MccRisk {
    values: HashMap<String, f32>,
}

impl MccRisk {
    pub fn standard() -> Self {
        let values = HashMap::from([
            ("5411".to_string(), 0.15),
            ("5812".to_string(), 0.30),
            ("5912".to_string(), 0.20),
            ("5944".to_string(), 0.45),
            ("7801".to_string(), 0.80),
            ("7802".to_string(), 0.75),
            ("7995".to_string(), 0.85),
            ("4511".to_string(), 0.35),
            ("5311".to_string(), 0.25),
            ("5999".to_string(), 0.50),
        ]);
        Self { values }
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let file = File::open(path)?;
        let values = serde_json::from_reader(BufReader::new(file)).map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid mcc_risk.json: {err}"),
            )
        })?;
        Ok(Self { values })
    }

    pub fn risk(&self, mcc: &str) -> f32 {
        self.values.get(mcc).copied().unwrap_or(0.5)
    }
}
