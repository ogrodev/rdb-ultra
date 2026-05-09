use std::{
    fs::File,
    io::{BufWriter, Write},
    mem::size_of,
    path::Path,
};

use memmap2::Mmap;
use thiserror::Error;

use crate::index::{decide_from_slices, NearestNeighbors, QuantizedVector, PADDED_DIMS};

const MAGIC: &[u8; 8] = b"RINHIDX1";
const HEADER_SIZE: usize = 4096;

#[derive(Debug, Error)]
pub enum BinaryIndexError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("index file is too small")]
    TooSmall,
    #[error("index magic mismatch")]
    BadMagic,
    #[error("index length does not match header: expected {expected} bytes, got {actual} bytes")]
    BadLength { expected: usize, actual: usize },
    #[error("index contains too many vectors for this platform: {0}")]
    TooManyVectors(u64),
}

pub struct MmapIndex {
    mmap: Mmap,
    len: usize,
}

impl MmapIndex {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, BinaryIndexError> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        if mmap.len() < HEADER_SIZE {
            return Err(BinaryIndexError::TooSmall);
        }
        if &mmap[0..8] != MAGIC {
            return Err(BinaryIndexError::BadMagic);
        }
        let mut len_bytes = [0_u8; 8];
        len_bytes.copy_from_slice(&mmap[8..16]);
        let len_u64 = u64::from_le_bytes(len_bytes);
        let len =
            usize::try_from(len_u64).map_err(|_| BinaryIndexError::TooManyVectors(len_u64))?;
        let expected = HEADER_SIZE + len * size_of::<QuantizedVector>() + len;
        if mmap.len() != expected {
            return Err(BinaryIndexError::BadLength {
                expected,
                actual: mmap.len(),
            });
        }
        Ok(Self { mmap, len })
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn vectors(&self) -> &[QuantizedVector] {
        let byte_len = self.len * size_of::<QuantizedVector>();
        let bytes = &self.mmap[HEADER_SIZE..HEADER_SIZE + byte_len];
        debug_assert_eq!(bytes.as_ptr().align_offset(size_of::<i16>()), 0);
        unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast::<QuantizedVector>(), self.len) }
    }

    fn labels(&self) -> &[u8] {
        let start = HEADER_SIZE + self.len * size_of::<QuantizedVector>();
        &self.mmap[start..start + self.len]
    }
}

impl NearestNeighbors for MmapIndex {
    fn decide_quantized(&self, query: &QuantizedVector) -> crate::index::Decision {
        decide_from_slices(query, self.vectors(), self.labels())
    }
}

pub struct HourBucketIndex {
    buckets: Vec<MmapIndex>,
}

impl HourBucketIndex {
    pub fn open_dir(path: impl AsRef<Path>) -> Result<Self, BinaryIndexError> {
        let mut buckets = Vec::with_capacity(24);
        for hour in 0..24 {
            let file_name = format!("support-h{hour:02}.idx");
            let mmap = MmapIndex::open(path.as_ref().join(file_name))?;
            buckets.push(mmap);
        }
        Ok(Self { buckets })
    }

    pub fn warmup(&self) {
        let query: QuantizedVector = [0; PADDED_DIMS];
        for bucket in &self.buckets {
            let decision = bucket.decide_quantized(&query);
            std::hint::black_box(decision);
        }
    }
}

impl NearestNeighbors for HourBucketIndex {
    fn decide_quantized(&self, query: &QuantizedVector) -> crate::index::Decision {
        let hour = quantized_hour_bucket(query[3]);
        self.buckets[hour].decide_quantized(query)
    }
}

fn quantized_hour_bucket(hour: i16) -> usize {
    let clamped = i32::from(hour).clamp(0, 10_000);
    ((clamped * 23 + 5_000) / 10_000) as usize
}

pub fn write_index(
    path: impl AsRef<Path>,
    vectors: &[QuantizedVector],
    labels: &[u8],
) -> Result<(), BinaryIndexError> {
    assert_eq!(
        vectors.len(),
        labels.len(),
        "vectors and labels must have the same length"
    );
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    let mut header = [0_u8; HEADER_SIZE];
    header[0..8].copy_from_slice(MAGIC);
    header[8..16].copy_from_slice(&(vectors.len() as u64).to_le_bytes());
    writer.write_all(&header)?;

    for vector in vectors {
        for value in vector.iter().take(PADDED_DIMS) {
            writer.write_all(&value.to_le_bytes())?;
        }
    }
    writer.write_all(labels)?;
    writer.flush()?;
    Ok(())
}
