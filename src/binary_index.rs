use std::{
    fs::File,
    io::{BufWriter, Write},
    mem::{align_of, size_of},
    path::Path,
};

use memmap2::Mmap;
use thiserror::Error;

use crate::index::{
    decide_from_slices, decide_kd_tree, decide_pruned_by_dim2, KdNode, NearestNeighbors,
    QuantizedVector, KD_LEAF, KD_LEAF_SIZE, PADDED_DIMS,
};

const MAGIC_V1: &[u8; 8] = b"RINHIDX1";
const MAGIC_V2: &[u8; 8] = b"RINHIDX2";
const MAGIC_V3: &[u8; 8] = b"RINHIDX3";
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
}

pub struct MmapIndex {
    mmap: Mmap,
    len: usize,
    format: IndexFormat,
    node_count: usize,
    nodes_start: usize,
}

impl MmapIndex {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, BinaryIndexError> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        if mmap.len() < HEADER_SIZE {
            return Err(BinaryIndexError::TooSmall);
        }
        let format = match &mmap[0..8] {
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
        let labels_end = checked_add(vectors_end, len, len_u64)?;
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

    pub fn vectors(&self) -> &[QuantizedVector] {
        let byte_len = self.len * size_of::<QuantizedVector>();
        let bytes = &self.mmap[HEADER_SIZE..HEADER_SIZE + byte_len];
        debug_assert_eq!(bytes.as_ptr().align_offset(size_of::<i16>()), 0);
        unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast::<QuantizedVector>(), self.len) }
    }

    pub fn labels(&self) -> &[u8] {
        let start = HEADER_SIZE + self.len * size_of::<QuantizedVector>();
        &self.mmap[start..start + self.len]
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

    let mut entries: Vec<(QuantizedVector, u8)> = vectors
        .iter()
        .copied()
        .zip(labels.iter().copied())
        .collect();
    let mut nodes = Vec::new();
    if !entries.is_empty() {
        build_kd_node(&mut entries, &mut nodes, 0, vectors.len())?;
    }

    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    let mut header = [0_u8; HEADER_SIZE];
    header[0..8].copy_from_slice(MAGIC_V3);
    header[8..16].copy_from_slice(&(entries.len() as u64).to_le_bytes());
    header[16..24].copy_from_slice(&(nodes.len() as u64).to_le_bytes());
    writer.write_all(&header)?;

    for (vector, _) in &entries {
        for value in vector.iter().take(PADDED_DIMS) {
            writer.write_all(&value.to_le_bytes())?;
        }
    }
    for (_, label) in &entries {
        writer.write_all(std::slice::from_ref(label))?;
    }

    let labels_end = HEADER_SIZE + entries.len() * size_of::<QuantizedVector>() + entries.len();
    for _ in labels_end..align_up(labels_end, align_of::<KdNode>()) {
        writer.write_all(&[0])?;
    }

    for node in &nodes {
        write_node(&mut writer, node)?;
    }
    writer.flush()?;
    Ok(())
}

fn build_kd_node(
    entries: &mut [(QuantizedVector, u8)],
    nodes: &mut Vec<KdNode>,
    start: usize,
    end: usize,
) -> Result<u32, BinaryIndexError> {
    let (min, max) = bounding_box(&entries[start..end]);
    let node_idx = u32::try_from(nodes.len())
        .map_err(|_| BinaryIndexError::TooManyVectors(nodes.len() as u64))?;
    nodes.push(KdNode {
        start: u32::try_from(start).map_err(|_| BinaryIndexError::TooManyVectors(start as u64))?,
        end: u32::try_from(end).map_err(|_| BinaryIndexError::TooManyVectors(end as u64))?,
        left: KD_LEAF,
        right: KD_LEAF,
        min,
        max,
    });

    if end - start > KD_LEAF_SIZE {
        let split_dim = widest_dimension(&min, &max);
        let mid = start + (end - start) / 2;
        entries[start..end]
            .select_nth_unstable_by_key(mid - start, |(vector, _)| vector[split_dim]);
        let left = build_kd_node(entries, nodes, start, mid)?;
        let right = build_kd_node(entries, nodes, mid, end)?;
        let node = &mut nodes[node_idx as usize];
        node.left = left;
        node.right = right;
    }

    Ok(node_idx)
}

fn bounding_box(
    entries: &[(QuantizedVector, u8)],
) -> ([i16; crate::index::DIMS], [i16; crate::index::DIMS]) {
    let mut min = [i16::MAX; crate::index::DIMS];
    let mut max = [i16::MIN; crate::index::DIMS];
    for (vector, _) in entries {
        for dim in 0..crate::index::DIMS {
            min[dim] = min[dim].min(vector[dim]);
            max[dim] = max[dim].max(vector[dim]);
        }
    }
    (min, max)
}

fn widest_dimension(min: &[i16; crate::index::DIMS], max: &[i16; crate::index::DIMS]) -> usize {
    (0..crate::index::DIMS)
        .max_by_key(|&dim| i32::from(max[dim]) - i32::from(min[dim]))
        .expect("vector has at least one dimension")
}

fn write_node(writer: &mut impl Write, node: &KdNode) -> Result<(), std::io::Error> {
    writer.write_all(&node.start.to_le_bytes())?;
    writer.write_all(&node.end.to_le_bytes())?;
    writer.write_all(&node.left.to_le_bytes())?;
    writer.write_all(&node.right.to_le_bytes())?;
    for value in node.min {
        writer.write_all(&value.to_le_bytes())?;
    }
    for value in node.max {
        writer.write_all(&value.to_le_bytes())?;
    }
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
