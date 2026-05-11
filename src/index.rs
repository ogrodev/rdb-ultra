use std::sync::Arc;

pub use crate::distance::squared_distance;
use crate::distance::squared_distance_fn;

pub const DIMS: usize = 14;
pub const PADDED_DIMS: usize = 16;
pub const QUANT_SCALE: f32 = 10_000.0;
pub type QuantizedVector = [i16; PADDED_DIMS];

pub const KD_LEAF: u32 = u32::MAX;
pub const KD_LEAF_SIZE: usize = 64;
const KD_STACK_CAPACITY: usize = 128;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KdNode {
    pub start: u32,
    pub end: u32,
    pub left: u32,
    pub right: u32,
    pub min: [i16; DIMS],
    pub max: [i16; DIMS],
}

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
    let mut best_indices = [usize::MAX; 5];
    let mut best_labels = [0_u8; 5];
    let distance_fn = squared_distance_fn();

    for (idx, vector) in vectors.iter().enumerate() {
        let distance = distance_fn(query, vector);
        if is_better_candidate(distance, idx, best_distances[4], best_indices[4]) {
            insert_best(
                distance,
                idx,
                labels[idx],
                &mut best_distances,
                &mut best_indices,
                &mut best_labels,
            );
        }
    }

    decision_from_best(&best_distances, &best_labels)
}

pub fn decide_pruned_by_dim2(
    query: &QuantizedVector,
    vectors: &[QuantizedVector],
    labels: &[u8],
) -> Decision {
    debug_assert_eq!(vectors.len(), labels.len());

    let mut best_distances = [i64::MAX; 5];
    let mut best_indices = [usize::MAX; 5];
    let mut best_labels = [0_u8; 5];
    let distance_fn = squared_distance_fn();

    let pivot = vectors.partition_point(|vector| vector[2] < query[2]);
    let mut left = pivot;
    let mut right = pivot;

    loop {
        let left_bound = if left == 0 {
            i64::MAX
        } else {
            dim2_distance(query, &vectors[left - 1])
        };
        let right_bound = if right == vectors.len() {
            i64::MAX
        } else {
            dim2_distance(query, &vectors[right])
        };

        if left_bound == i64::MAX && right_bound == i64::MAX {
            break;
        }

        if left_bound <= right_bound {
            if left_bound > best_distances[4] {
                break;
            }
            left -= 1;
            let distance = distance_fn(query, &vectors[left]);
            if is_better_candidate(distance, left, best_distances[4], best_indices[4]) {
                insert_best(
                    distance,
                    left,
                    labels[left],
                    &mut best_distances,
                    &mut best_indices,
                    &mut best_labels,
                );
            }
        } else {
            if right_bound > best_distances[4] {
                break;
            }
            let distance = distance_fn(query, &vectors[right]);
            if is_better_candidate(distance, right, best_distances[4], best_indices[4]) {
                insert_best(
                    distance,
                    right,
                    labels[right],
                    &mut best_distances,
                    &mut best_indices,
                    &mut best_labels,
                );
            }
            right += 1;
        }
    }

    decision_from_best(&best_distances, &best_labels)
}

pub fn decide_kd_tree(
    query: &QuantizedVector,
    vectors: &[QuantizedVector],
    labels: &[u8],
    nodes: &[KdNode],
) -> Decision {
    debug_assert_eq!(vectors.len(), labels.len());

    if nodes.is_empty() {
        return Decision::safe_fallback();
    }

    let mut best_distances = [i64::MAX; 5];
    let mut best_indices = [usize::MAX; 5];
    let mut best_labels = [0_u8; 5];
    let distance_fn = squared_distance_fn();
    let mut stack = [0_usize; KD_STACK_CAPACITY];
    let mut stack_len = 1_usize;

    while stack_len > 0 {
        stack_len -= 1;
        let node_idx = stack[stack_len];
        let node = &nodes[node_idx];
        if kd_node_bound(query, node) > best_distances[4] {
            continue;
        }

        if node.left == KD_LEAF {
            for idx in node.start as usize..node.end as usize {
                let distance = distance_fn(query, &vectors[idx]);
                if is_better_candidate(distance, idx, best_distances[4], best_indices[4]) {
                    insert_best(
                        distance,
                        idx,
                        labels[idx],
                        &mut best_distances,
                        &mut best_indices,
                        &mut best_labels,
                    );
                }
            }
            continue;
        }

        let left_idx = node.left as usize;
        let right_idx = node.right as usize;
        let left_bound = kd_node_bound(query, &nodes[left_idx]);
        let right_bound = kd_node_bound(query, &nodes[right_idx]);

        if left_bound <= right_bound {
            if right_bound <= best_distances[4] {
                push_kd_node(&mut stack, &mut stack_len, right_idx);
            }
            if left_bound <= best_distances[4] {
                push_kd_node(&mut stack, &mut stack_len, left_idx);
            }
        } else {
            if left_bound <= best_distances[4] {
                push_kd_node(&mut stack, &mut stack_len, left_idx);
            }
            if right_bound <= best_distances[4] {
                push_kd_node(&mut stack, &mut stack_len, right_idx);
            }
        }
    }

    decision_from_best(&best_distances, &best_labels)
}

pub fn decide_from_soa(
    query: &QuantizedVector,
    dimensions: [&[i16]; DIMS],
    labels: &[u8],
) -> Decision {
    debug_assert!(dimensions
        .iter()
        .all(|dimension| dimension.len() == labels.len()));

    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("avx2") {
            return unsafe { decide_from_soa_avx2(query, &dimensions, labels) };
        }
    }

    decide_from_soa_scalar(query, &dimensions, labels)
}

fn decide_from_soa_scalar(
    query: &QuantizedVector,
    dimensions: &[&[i16]; DIMS],
    labels: &[u8],
) -> Decision {
    let mut best_distances = [i64::MAX; 5];
    let mut best_indices = [usize::MAX; 5];
    let mut best_labels = [0_u8; 5];

    for (idx, &label) in labels.iter().enumerate() {
        let distance = squared_distance_soa_at(query, dimensions, idx);
        if is_better_candidate(distance, idx, best_distances[4], best_indices[4]) {
            insert_best(
                distance,
                idx,
                label,
                &mut best_distances,
                &mut best_indices,
                &mut best_labels,
            );
        }
    }

    decision_from_best(&best_distances, &best_labels)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn decide_from_soa_avx2(
    query: &QuantizedVector,
    dimensions: &[&[i16]; DIMS],
    labels: &[u8],
) -> Decision {
    use std::arch::x86_64::{
        __m128i, __m256i, _mm256_add_epi32, _mm256_cvtepi16_epi32, _mm256_mullo_epi32,
        _mm256_setzero_si256, _mm256_storeu_si256, _mm_loadu_si128, _mm_set1_epi16, _mm_sub_epi16,
    };

    let mut best_distances = [i64::MAX; 5];
    let mut best_indices = [usize::MAX; 5];
    let mut best_labels = [0_u8; 5];
    let mut idx = 0_usize;
    while idx + 8 <= labels.len() {
        let mut distances = _mm256_setzero_si256();
        for dim in 0..DIMS {
            let values = _mm_loadu_si128(dimensions[dim].as_ptr().add(idx).cast::<__m128i>());
            let query_values = _mm_set1_epi16(query[dim]);
            let diff_i16 = _mm_sub_epi16(query_values, values);
            let diff_i32 = _mm256_cvtepi16_epi32(diff_i16);
            let squared = _mm256_mullo_epi32(diff_i32, diff_i32);
            distances = _mm256_add_epi32(distances, squared);
        }

        let mut lanes = [0_i32; 8];
        _mm256_storeu_si256(lanes.as_mut_ptr().cast::<__m256i>(), distances);
        for (lane, distance) in lanes.iter().enumerate() {
            debug_assert!(*distance >= 0, "SOA AVX2 distance overflowed i32");
            let candidate_idx = idx + lane;
            let distance = i64::from(*distance);
            if is_better_candidate(distance, candidate_idx, best_distances[4], best_indices[4]) {
                insert_best(
                    distance,
                    candidate_idx,
                    labels[candidate_idx],
                    &mut best_distances,
                    &mut best_indices,
                    &mut best_labels,
                );
            }
        }

        idx += 8;
    }

    while idx < labels.len() {
        let distance = squared_distance_soa_at(query, dimensions, idx);
        if is_better_candidate(distance, idx, best_distances[4], best_indices[4]) {
            insert_best(
                distance,
                idx,
                labels[idx],
                &mut best_distances,
                &mut best_indices,
                &mut best_labels,
            );
        }
        idx += 1;
    }

    decision_from_best(&best_distances, &best_labels)
}

#[inline(always)]
fn squared_distance_soa_at(
    query: &QuantizedVector,
    dimensions: &[&[i16]; DIMS],
    idx: usize,
) -> i64 {
    let mut sum = 0_i64;
    for dim in 0..DIMS {
        let diff = i32::from(query[dim]) - i32::from(dimensions[dim][idx]);
        sum += i64::from(diff * diff);
    }
    sum
}

#[inline(always)]
fn push_kd_node(stack: &mut [usize; KD_STACK_CAPACITY], stack_len: &mut usize, node_idx: usize) {
    debug_assert!(*stack_len < stack.len());
    stack[*stack_len] = node_idx;
    *stack_len += 1;
}

#[inline(always)]
fn kd_node_bound(query: &QuantizedVector, node: &KdNode) -> i64 {
    let mut sum = 0_i64;
    for (dim, &query_value) in query.iter().enumerate().take(DIMS) {
        let nearest = if query_value < node.min[dim] {
            node.min[dim]
        } else if query_value > node.max[dim] {
            node.max[dim]
        } else {
            query_value
        };
        let diff = i32::from(query_value) - i32::from(nearest);
        sum += i64::from(diff * diff);
    }
    sum
}

#[inline(always)]
fn dim2_distance(query: &QuantizedVector, vector: &QuantizedVector) -> i64 {
    let diff = i32::from(query[2]) - i32::from(vector[2]);
    i64::from(diff * diff)
}

#[inline(always)]
fn is_better_candidate(
    distance: i64,
    index: usize,
    worst_distance: i64,
    worst_index: usize,
) -> bool {
    distance < worst_distance || (distance == worst_distance && index < worst_index)
}

#[inline(always)]
fn insert_best(
    distance: i64,
    index: usize,
    label: u8,
    best_distances: &mut [i64; 5],
    best_indices: &mut [usize; 5],
    best_labels: &mut [u8; 5],
) {
    let mut position = 4;
    while position > 0
        && is_better_candidate(
            distance,
            index,
            best_distances[position - 1],
            best_indices[position - 1],
        )
    {
        best_distances[position] = best_distances[position - 1];
        best_indices[position] = best_indices[position - 1];
        best_labels[position] = best_labels[position - 1];
        position -= 1;
    }
    best_distances[position] = distance;
    best_indices[position] = index;
    best_labels[position] = label;
}

fn decision_from_best(best_distances: &[i64; 5], best_labels: &[u8; 5]) -> Decision {
    let fraud_count = best_labels
        .iter()
        .zip(best_distances.iter())
        .filter(|(_, distance)| **distance != i64::MAX)
        .map(|(label, _)| *label)
        .sum::<u8>();

    Decision::from_fraud_count(fraud_count)
}
