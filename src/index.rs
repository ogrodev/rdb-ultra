use std::sync::Arc;

pub const DIMS: usize = 14;
pub const PADDED_DIMS: usize = 16;
pub const QUANT_SCALE: f32 = 10_000.0;
pub type QuantizedVector = [i16; PADDED_DIMS];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Decision {
    pub approved: bool,
    pub fraud_score: f32,
    pub fraud_count: u8,
}

impl Decision {
    pub fn from_fraud_count(fraud_count: u8) -> Self {
        let fraud_score = fraud_count as f32 / 5.0;
        Self {
            approved: fraud_score < 0.6,
            fraud_score,
            fraud_count,
        }
    }

    pub fn safe_fallback() -> Self {
        Self::from_fraud_count(0)
    }
}

#[derive(Debug, Clone)]
pub struct ReferenceRecord {
    pub vector: [f32; DIMS],
    pub fraud: bool,
}

impl ReferenceRecord {
    pub fn new(vector: [f32; DIMS], fraud: bool) -> Self {
        Self { vector, fraud }
    }
}

pub trait NearestNeighbors: Send + Sync {
    fn decide_quantized(&self, query: &QuantizedVector) -> Decision;

    fn decide(&self, query: &[f32; DIMS]) -> Decision {
        let query = quantize(query);
        self.decide_quantized(&query)
    }
}

impl<T: NearestNeighbors + ?Sized> NearestNeighbors for Arc<T> {
    fn decide_quantized(&self, query: &QuantizedVector) -> Decision {
        (**self).decide_quantized(query)
    }
}

#[derive(Debug, Clone)]
pub struct ReferenceIndex {
    vectors: Vec<QuantizedVector>,
    labels: Vec<u8>,
}

impl ReferenceIndex {
    pub fn from_records(records: Vec<ReferenceRecord>) -> Self {
        let mut vectors = Vec::with_capacity(records.len());
        let mut labels = Vec::with_capacity(records.len());
        for record in records {
            vectors.push(quantize(&record.vector));
            labels.push(u8::from(record.fraud));
        }
        Self { vectors, labels }
    }

    pub fn from_quantized(vectors: Vec<QuantizedVector>, labels: Vec<u8>) -> Self {
        assert_eq!(
            vectors.len(),
            labels.len(),
            "vectors and labels must have the same length"
        );
        Self { vectors, labels }
    }

    pub fn len(&self) -> usize {
        self.vectors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }

    pub fn vectors(&self) -> &[QuantizedVector] {
        &self.vectors
    }

    pub fn labels(&self) -> &[u8] {
        &self.labels
    }
}

impl NearestNeighbors for ReferenceIndex {
    fn decide_quantized(&self, query: &QuantizedVector) -> Decision {
        decide_from_slices(query, &self.vectors, &self.labels)
    }
}

pub fn quantize(vector: &[f32; DIMS]) -> QuantizedVector {
    let mut quantized = [0_i16; PADDED_DIMS];
    for idx in 0..DIMS {
        let value = vector[idx];
        let normalized = if value < 0.0 {
            -1.0
        } else {
            value.clamp(0.0, 1.0)
        };
        quantized[idx] = (normalized * QUANT_SCALE).round() as i16;
    }
    quantized
}

pub fn decide_from_slices(
    query: &QuantizedVector,
    vectors: &[QuantizedVector],
    labels: &[u8],
) -> Decision {
    debug_assert_eq!(vectors.len(), labels.len());

    let mut best_distances = [i64::MAX; 5];
    let mut best_labels = [0_u8; 5];

    for (idx, vector) in vectors.iter().enumerate() {
        let distance = squared_distance(query, vector);
        if distance < best_distances[4] {
            insert_best(distance, labels[idx], &mut best_distances, &mut best_labels);
        }
    }

    let fraud_count = best_labels
        .iter()
        .zip(best_distances.iter())
        .filter(|(_, distance)| **distance != i64::MAX)
        .map(|(label, _)| *label)
        .sum::<u8>();

    Decision::from_fraud_count(fraud_count)
}

#[inline(always)]
pub fn squared_distance(left: &QuantizedVector, right: &QuantizedVector) -> i64 {
    let mut sum = 0_i64;
    for idx in 0..DIMS {
        let diff = i32::from(left[idx]) - i32::from(right[idx]);
        sum += i64::from(diff * diff);
    }
    sum
}

#[inline(always)]
fn insert_best(distance: i64, label: u8, best_distances: &mut [i64; 5], best_labels: &mut [u8; 5]) {
    let mut position = 4;
    while position > 0 && distance < best_distances[position - 1] {
        best_distances[position] = best_distances[position - 1];
        best_labels[position] = best_labels[position - 1];
        position -= 1;
    }
    best_distances[position] = distance;
    best_labels[position] = label;
}
