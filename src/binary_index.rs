use std::{
    fs::File,
    io::{BufWriter, Write},
    mem::{align_of, size_of},
    path::Path,
};

use memmap2::Mmap;
use thiserror::Error;

use crate::index::{
    decide_from_slices, decide_from_soa, decide_kd_tree, decide_pruned_by_dim2, KdNode,
    NearestNeighbors, QuantizedVector, PADDED_DIMS,
};

const MAGIC_V1: &[u8; 8] = b"RINHIDX1";
const MAGIC_V2: &[u8; 8] = b"RINHIDX2";
const MAGIC_V3: &[u8; 8] = b"RINHIDX3";
const MAGIC_V4: &[u8; 8] = b"RINHIDX4";
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexFormat {
    Legacy,
    SortedByDim2,
    KdTree,
    Soa,
}

pub struct MmapIndex {
    mmap: Mmap,
    len: usize,
    format: IndexFormat,
    node_count: usize,
    nodes_start: usize,
    soa_start: usize,
    labels_start: usize,
}

impl MmapIndex {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, BinaryIndexError> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        if mmap.len() < HEADER_SIZE {
            return Err(BinaryIndexError::TooSmall);
        }
        let format = match &mmap[0..8] {
            magic if magic == MAGIC_V4 => IndexFormat::Soa,
            magic if magic == MAGIC_V3 => IndexFormat::KdTree,
            magic if magic == MAGIC_V2 => IndexFormat::SortedByDim2,
            magic if magic == MAGIC_V1 => IndexFormat::Legacy,
            _ => return Err(BinaryIndexError::BadMagic),
        };
        let mut len_bytes = [0_u8; 8];
        len_bytes.copy_from_slice(&mmap[8..16]);
        let len_u64 = u64::from_le_bytes(len_bytes);
        let len =
            usize::try_from(len_u64).map_err(|_| BinaryIndexError::TooManyVectors(len_u64))?;

        let node_count = if format == IndexFormat::KdTree {
            let mut node_count_bytes = [0_u8; 8];
            node_count_bytes.copy_from_slice(&mmap[16..24]);
            let node_count_u64 = u64::from_le_bytes(node_count_bytes);
            usize::try_from(node_count_u64)
                .map_err(|_| BinaryIndexError::TooManyVectors(node_count_u64))?
        } else {
            0
        };

        let vectors_len = checked_mul(len, size_of::<QuantizedVector>(), len_u64)?;
        let vectors_end = checked_add(HEADER_SIZE, vectors_len, len_u64)?;
        let soa_start = vectors_end;
        let soa_len = if format == IndexFormat::Soa {
            checked_mul(
                checked_mul(len, crate::index::DIMS, len_u64)?,
                size_of::<i16>(),
                len_u64,
            )?
        } else {
            0
        };
        let labels_start = checked_add(soa_start, soa_len, len_u64)?;
        let labels_end = checked_add(labels_start, len, len_u64)?;
        let nodes_start = if format == IndexFormat::KdTree {
            align_up(labels_end, align_of::<KdNode>())
        } else {
            labels_end
        };
        let nodes_len = checked_mul(node_count, size_of::<KdNode>(), len_u64)?;
        let expected = checked_add(nodes_start, nodes_len, len_u64)?;
        if mmap.len() != expected {
            return Err(BinaryIndexError::BadLength {
                expected,
                actual: mmap.len(),
            });
        }
        Ok(Self {
            mmap,
            len,
            format,
            node_count,
            nodes_start,
            soa_start,
            labels_start,
        })
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn supports_pruned_search(&self) -> bool {
        self.format == IndexFormat::SortedByDim2
    }

    pub fn supports_kd_tree(&self) -> bool {
        self.format == IndexFormat::KdTree
    }

    pub fn supports_soa(&self) -> bool {
        self.format == IndexFormat::Soa
    }

    pub fn vectors(&self) -> &[QuantizedVector] {
        let byte_len = self.len * size_of::<QuantizedVector>();
        let bytes = &self.mmap[HEADER_SIZE..HEADER_SIZE + byte_len];
        debug_assert_eq!(bytes.as_ptr().align_offset(size_of::<i16>()), 0);
        unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast::<QuantizedVector>(), self.len) }
    }

    pub fn labels(&self) -> &[u8] {
        &self.mmap[self.labels_start..self.labels_start + self.len]
    }

    pub fn dimensions(&self) -> [&[i16]; crate::index::DIMS] {
        std::array::from_fn(|dim| self.dimension(dim))
    }

    fn dimension(&self, dim: usize) -> &[i16] {
        let dim_len = self.len * size_of::<i16>();
        let start = self.soa_start + dim * dim_len;
        let bytes = &self.mmap[start..start + dim_len];
        debug_assert_eq!(bytes.as_ptr().align_offset(size_of::<i16>()), 0);
        unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast::<i16>(), self.len) }
    }

    pub fn nodes(&self) -> &[KdNode] {
        if self.node_count == 0 {
            return &[];
        }
        let byte_len = self.node_count * size_of::<KdNode>();
        let bytes = &self.mmap[self.nodes_start..self.nodes_start + byte_len];
        debug_assert_eq!(bytes.as_ptr().align_offset(align_of::<KdNode>()), 0);
        unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast::<KdNode>(), self.node_count) }
    }
}

impl NearestNeighbors for MmapIndex {
    fn decide_quantized(&self, query: &QuantizedVector) -> crate::index::Decision {
        let vectors = self.vectors();
        let labels = self.labels();
        match self.format {
            IndexFormat::Soa => decide_from_soa(query, self.dimensions(), labels),
            IndexFormat::KdTree => decide_kd_tree(query, vectors, labels, self.nodes()),
            IndexFormat::SortedByDim2 => decide_pruned_by_dim2(query, vectors, labels),
            IndexFormat::Legacy => decide_from_slices(query, vectors, labels),
        }
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

    let entries: Vec<(QuantizedVector, u8)> = vectors
        .iter()
        .copied()
        .zip(labels.iter().copied())
        .collect();

    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    let mut header = [0_u8; HEADER_SIZE];
    header[0..8].copy_from_slice(MAGIC_V4);
    header[8..16].copy_from_slice(&(entries.len() as u64).to_le_bytes());
    writer.write_all(&header)?;

    for (vector, _) in &entries {
        for value in vector.iter().take(PADDED_DIMS) {
            writer.write_all(&value.to_le_bytes())?;
        }
    }
    for dim in 0..crate::index::DIMS {
        for (vector, _) in &entries {
            writer.write_all(&vector[dim].to_le_bytes())?;
        }
    }
    for (_, label) in &entries {
        writer.write_all(std::slice::from_ref(label))?;
    }
    writer.flush()?;
    Ok(())
}

fn checked_mul(left: usize, right: usize, len: u64) -> Result<usize, BinaryIndexError> {
    left.checked_mul(right)
        .ok_or(BinaryIndexError::TooManyVectors(len))
}

fn checked_add(left: usize, right: usize, len: u64) -> Result<usize, BinaryIndexError> {
    left.checked_add(right)
        .ok_or(BinaryIndexError::TooManyVectors(len))
}

fn align_up(value: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    (value + align - 1) & !(align - 1)
}
