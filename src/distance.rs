use std::sync::OnceLock;

use crate::index::QuantizedVector;

pub type DistanceFn = fn(&QuantizedVector, &QuantizedVector) -> i64;

pub fn squared_distance_fn() -> DistanceFn {
    static DISTANCE_FN: OnceLock<DistanceFn> = OnceLock::new();
    *DISTANCE_FN.get_or_init(select_distance_fn)
}

pub fn squared_distance(left: &QuantizedVector, right: &QuantizedVector) -> i64 {
    squared_distance_fn()(left, right)
}

fn select_distance_fn() -> DistanceFn {
    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("avx2") {
            return squared_distance_avx2;
        }
    }
    squared_distance_scalar
}

#[inline(always)]
pub fn squared_distance_scalar(left: &QuantizedVector, right: &QuantizedVector) -> i64 {
    let mut sum = 0_i64;
    for idx in 0..crate::index::DIMS {
        let diff = i32::from(left[idx]) - i32::from(right[idx]);
        sum += i64::from(diff * diff);
    }
    sum
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn squared_distance_avx2(left: &QuantizedVector, right: &QuantizedVector) -> i64 {
    unsafe { squared_distance_avx2_inner(left, right) }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn squared_distance_avx2_inner(left: &QuantizedVector, right: &QuantizedVector) -> i64 {
    use std::arch::x86_64::{
        __m256i, _mm256_add_epi64, _mm256_cvtepi32_epi64, _mm256_extracti128_si256,
        _mm256_loadu_si256, _mm256_madd_epi16, _mm256_storeu_si256, _mm256_sub_epi16,
    };

    let left = _mm256_loadu_si256(left.as_ptr().cast::<__m256i>());
    let right = _mm256_loadu_si256(right.as_ptr().cast::<__m256i>());
    let diff = _mm256_sub_epi16(left, right);
    let pair_sums = _mm256_madd_epi16(diff, diff);

    let low_i64 = _mm256_cvtepi32_epi64(_mm256_extracti128_si256::<0>(pair_sums));
    let high_i64 = _mm256_cvtepi32_epi64(_mm256_extracti128_si256::<1>(pair_sums));
    let sums = _mm256_add_epi64(low_i64, high_i64);

    let mut lanes = [0_i64; 4];
    _mm256_storeu_si256(lanes.as_mut_ptr().cast::<__m256i>(), sums);
    lanes.iter().sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{DIMS, PADDED_DIMS};

    #[test]
    fn dispatch_matches_scalar_for_random_pairs() {
        let mut seed = 0x6176_7832_2d64_6973_u64;
        for _ in 0..10_000 {
            let left = random_vector(&mut seed);
            let right = random_vector(&mut seed);

            assert_eq!(
                squared_distance(&left, &right),
                squared_distance_scalar(&left, &right)
            );
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn avx2_matches_scalar_for_random_pairs_when_available() {
        if !std::is_x86_feature_detected!("avx2") {
            eprintln!("skipping AVX2 distance parity check: AVX2 is not available");
            return;
        }

        let mut seed = 0x6861_7377_656c_6c32_u64;
        for _ in 0..10_000 {
            let left = random_vector(&mut seed);
            let right = random_vector(&mut seed);
            let expected = squared_distance_scalar(&left, &right);
            let actual = unsafe { squared_distance_avx2_inner(&left, &right) };

            assert_eq!(actual, expected);
        }
    }

    fn random_vector(seed: &mut u64) -> QuantizedVector {
        let mut vector = [0_i16; PADDED_DIMS];
        for value in vector.iter_mut().take(DIMS) {
            *value = next_quantized(seed);
        }
        vector
    }

    fn next_quantized(seed: &mut u64) -> i16 {
        (next_u32(seed) % 20_001) as i16 - 10_000
    }

    fn next_u32(seed: &mut u64) -> u32 {
        *seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (*seed >> 32) as u32
    }
}
