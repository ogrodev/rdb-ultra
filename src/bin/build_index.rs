use std::{
    env,
    fs::{self, File},
    io::{BufReader, Read},
    path::{Path, PathBuf},
};

use flate2::read::GzDecoder;
use rinha_backend_v2::{
    binary_index::{write_index, MmapIndex},
    index::{quantize, QuantizedVector, DIMS},
};
use serde::{
    de::{SeqAccess, Visitor},
    Deserialize, Deserializer,
};

#[derive(Debug, Deserialize)]
struct ReferenceJson {
    vector: Vec<f32>,
    label: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    match args.as_slice() {
        [command, dir] if command == Path::new("--sort-hour-buckets") => {
            sort_hour_buckets(dir)?;
        }
        [input, output] => {
            let file = File::open(input)?;
            let decoder = GzDecoder::new(BufReader::new(file));
            let (vectors, labels) = read_references(decoder)?;
            write_index(output, &vectors, &labels)?;
            eprintln!("indexed {} reference vectors", vectors.len());
        }
        _ => {
            return Err(
                "usage: build-index <references.json.gz> <output.ridx> | build-index --sort-hour-buckets <model/hour>"
                    .into(),
            );
        }
    }
    Ok(())
}

fn sort_hour_buckets(dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    for hour in 0..24 {
        let path = dir.join(format!("support-h{hour:02}.idx"));
        let (vectors, labels) = {
            let index = MmapIndex::open(&path)?;
            (index.vectors().to_vec(), index.labels().to_vec())
        };
        let tmp_path = dir.join(format!("support-h{hour:02}.idx.tmp"));
        write_index(&tmp_path, &vectors, &labels)?;
        fs::rename(&tmp_path, &path)?;
        eprintln!("sorted {} vectors in {}", vectors.len(), path.display());
    }
    Ok(())
}

fn read_references<R: Read>(
    reader: R,
) -> Result<(Vec<QuantizedVector>, Vec<u8>), serde_json::Error> {
    struct ReferencesVisitor;

    impl<'de> Visitor<'de> for ReferencesVisitor {
        type Value = (Vec<QuantizedVector>, Vec<u8>);

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("an array of reference vectors")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let capacity = seq.size_hint().unwrap_or(0);
            let mut vectors = Vec::with_capacity(capacity);
            let mut labels = Vec::with_capacity(capacity);

            while let Some(reference) = seq.next_element::<ReferenceJson>()? {
                if reference.vector.len() != DIMS {
                    return Err(serde::de::Error::custom(format!(
                        "reference vector must have {DIMS} dimensions, got {}",
                        reference.vector.len()
                    )));
                }
                let mut vector = [0_f32; DIMS];
                vector.copy_from_slice(&reference.vector);
                vectors.push(quantize(&vector));
                labels.push(match reference.label.as_str() {
                    "fraud" => 1,
                    "legit" => 0,
                    other => {
                        return Err(serde::de::Error::custom(format!(
                            "unknown reference label: {other}"
                        )));
                    }
                });
            }
            Ok((vectors, labels))
        }
    }

    let mut deserializer = serde_json::Deserializer::from_reader(reader);
    deserializer.deserialize_seq(ReferencesVisitor)
}
