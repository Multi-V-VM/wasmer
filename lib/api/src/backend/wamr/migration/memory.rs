//! Memory serialization for MVVM checkpoints.
//!
//! This module provides utilities for serializing and deserializing
//! WebAssembly linear memory for checkpoint/restore operations.
//!
//! # Key Design Principles
//!
//! 1. **Pointer Abstraction**: WebAssembly pointers are offsets into linear memory,
//!    not absolute addresses. This makes them inherently portable across architectures.
//!
//! 2. **Compression**: Memory is compressed using LZ4 for efficient transfer.
//!    This is particularly effective for sparse memory regions.
//!
//! 3. **Incremental Updates**: For periodic checkpoints, we support capturing
//!    only changed memory regions to reduce checkpoint size.

use super::MigrationError;
use serde::{Deserialize, Serialize};

/// Represents a region of linear memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryRegion {
    /// Start offset in bytes from memory base
    pub start: u64,
    /// End offset (exclusive) in bytes from memory base
    pub end: u64,
}

impl MemoryRegion {
    /// Creates a new memory region.
    pub fn new(start: u64, end: u64) -> Self {
        debug_assert!(start <= end);
        Self { start, end }
    }

    /// Returns the size of the region in bytes.
    pub fn size(&self) -> u64 {
        self.end - self.start
    }

    /// Returns true if this region overlaps with another.
    pub fn overlaps(&self, other: &Self) -> bool {
        self.start < other.end && other.start < self.end
    }

    /// Merges two overlapping or adjacent regions.
    pub fn merge(&self, other: &Self) -> Option<Self> {
        if self.end >= other.start && other.end >= self.start {
            Some(Self {
                start: self.start.min(other.start),
                end: self.end.max(other.end),
            })
        } else {
            None
        }
    }
}

/// Serialized memory snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySnapshot {
    /// Total memory size in bytes
    pub total_size: u64,
    /// Number of WebAssembly pages (64KB each)
    pub pages: u32,
    /// Compressed memory data
    pub data: Vec<u8>,
    /// Whether the data is compressed
    pub compressed: bool,
}

impl MemorySnapshot {
    /// WebAssembly page size in bytes (64KB)
    pub const PAGE_SIZE: u64 = 65536;

    /// Creates a new memory snapshot from raw memory data.
    pub fn from_raw(data: &[u8], compress: bool) -> Result<Self, MigrationError> {
        let total_size = data.len() as u64;
        let pages = ((total_size + Self::PAGE_SIZE - 1) / Self::PAGE_SIZE) as u32;

        let snapshot_data = if compress {
            lz4_flex::compress_prepend_size(data)
        } else {
            data.to_vec()
        };

        Ok(Self {
            total_size,
            pages,
            data: snapshot_data,
            compressed: compress,
        })
    }

    /// Restores the raw memory data from the snapshot.
    pub fn to_raw(&self) -> Result<Vec<u8>, MigrationError> {
        if self.compressed {
            lz4_flex::decompress_size_prepended(&self.data)
                .map_err(|e| MigrationError::SerializationError(e.to_string()))
        } else {
            Ok(self.data.clone())
        }
    }

    /// Returns the compression ratio (compressed size / original size).
    pub fn compression_ratio(&self) -> f64 {
        if self.total_size == 0 {
            1.0
        } else {
            self.data.len() as f64 / self.total_size as f64
        }
    }
}

/// Incremental memory delta for efficient periodic checkpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryDelta {
    /// Base checkpoint ID this delta applies to
    pub base_checkpoint_id: u64,
    /// Changed regions with their new data
    pub changes: Vec<MemoryChange>,
    /// Total size of all changes
    pub total_change_size: u64,
}

/// A single memory change within a delta.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryChange {
    /// Region that changed
    pub region: MemoryRegion,
    /// Compressed data for this region
    pub data: Vec<u8>,
}

impl MemoryDelta {
    /// Creates a new empty delta.
    pub fn new(base_checkpoint_id: u64) -> Self {
        Self {
            base_checkpoint_id,
            changes: Vec::new(),
            total_change_size: 0,
        }
    }

    /// Adds a change to the delta.
    pub fn add_change(&mut self, region: MemoryRegion, data: &[u8]) {
        let compressed = lz4_flex::compress_prepend_size(data);
        self.total_change_size += region.size();
        self.changes.push(MemoryChange {
            region,
            data: compressed,
        });
    }

    /// Returns true if this delta is empty.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Applies this delta to a base memory snapshot.
    pub fn apply_to(&self, base: &mut [u8]) -> Result<(), MigrationError> {
        for change in &self.changes {
            let data = lz4_flex::decompress_size_prepended(&change.data)
                .map_err(|e| MigrationError::SerializationError(e.to_string()))?;

            let start = change.region.start as usize;
            let end = change.region.end as usize;

            if end > base.len() {
                return Err(MigrationError::RestoreFailed(format!(
                    "Memory region {}..{} exceeds base memory size {}",
                    start,
                    end,
                    base.len()
                )));
            }

            base[start..end].copy_from_slice(&data);
        }
        Ok(())
    }
}

/// Memory hash for change detection.
///
/// Uses Blake3 for fast hashing of memory regions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryHash {
    /// Hash of the memory contents
    pub hash: [u8; 32],
    /// Size of the hashed region
    pub size: u64,
}

impl MemoryHash {
    /// Computes a hash of the given memory region.
    pub fn compute(data: &[u8]) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // Use a simple hash for now (in production, use Blake3)
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        let hash_value = hasher.finish();

        let mut hash = [0u8; 32];
        hash[0..8].copy_from_slice(&hash_value.to_le_bytes());

        Self {
            hash,
            size: data.len() as u64,
        }
    }

    /// Returns true if two hashes are equal.
    pub fn matches(&self, other: &Self) -> bool {
        self.hash == other.hash && self.size == other.size
    }
}

/// Serializes memory with region-based change detection.
pub struct MemorySerializer {
    /// Size of each region for hashing (default 512 bytes)
    region_size: usize,
    /// Previous region hashes for change detection
    previous_hashes: Vec<MemoryHash>,
}

impl MemorySerializer {
    /// Default region size for change detection (512 bytes)
    pub const DEFAULT_REGION_SIZE: usize = 512;

    /// Creates a new memory serializer.
    pub fn new() -> Self {
        Self::with_region_size(Self::DEFAULT_REGION_SIZE)
    }

    /// Creates a new memory serializer with custom region size.
    pub fn with_region_size(region_size: usize) -> Self {
        Self {
            region_size,
            previous_hashes: Vec::new(),
        }
    }

    /// Creates a full memory snapshot.
    pub fn create_snapshot(&mut self, memory: &[u8]) -> Result<MemorySnapshot, MigrationError> {
        // Update hashes for future delta detection
        self.update_hashes(memory);

        // Create compressed snapshot
        MemorySnapshot::from_raw(memory, true)
    }

    /// Creates an incremental delta from the previous checkpoint.
    pub fn create_delta(
        &mut self,
        memory: &[u8],
        base_checkpoint_id: u64,
    ) -> Result<MemoryDelta, MigrationError> {
        let mut delta = MemoryDelta::new(base_checkpoint_id);

        if self.previous_hashes.is_empty() {
            // No previous checkpoint, need full snapshot
            delta.add_change(MemoryRegion::new(0, memory.len() as u64), memory);
        } else {
            // Find changed regions
            let num_regions = (memory.len() + self.region_size - 1) / self.region_size;

            for i in 0..num_regions {
                let start = i * self.region_size;
                let end = (start + self.region_size).min(memory.len());
                let region_data = &memory[start..end];

                let current_hash = MemoryHash::compute(region_data);

                let changed = i >= self.previous_hashes.len()
                    || !current_hash.matches(&self.previous_hashes[i]);

                if changed {
                    delta.add_change(MemoryRegion::new(start as u64, end as u64), region_data);
                }
            }
        }

        // Update hashes
        self.update_hashes(memory);

        Ok(delta)
    }

    /// Updates stored hashes from current memory state.
    fn update_hashes(&mut self, memory: &[u8]) {
        let num_regions = (memory.len() + self.region_size - 1) / self.region_size;
        self.previous_hashes.clear();
        self.previous_hashes.reserve(num_regions);

        for i in 0..num_regions {
            let start = i * self.region_size;
            let end = (start + self.region_size).min(memory.len());
            self.previous_hashes
                .push(MemoryHash::compute(&memory[start..end]));
        }
    }
}

impl Default for MemorySerializer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_region() {
        let r1 = MemoryRegion::new(0, 100);
        let r2 = MemoryRegion::new(50, 150);
        let r3 = MemoryRegion::new(200, 300);

        assert!(r1.overlaps(&r2));
        assert!(!r1.overlaps(&r3));

        let merged = r1.merge(&r2).unwrap();
        assert_eq!(merged.start, 0);
        assert_eq!(merged.end, 150);
    }

    #[test]
    fn test_memory_snapshot_roundtrip() {
        let data = vec![0u8; 10000];
        let snapshot = MemorySnapshot::from_raw(&data, true).unwrap();

        assert!(snapshot.compressed);
        assert!(snapshot.data.len() < data.len());

        let restored = snapshot.to_raw().unwrap();
        assert_eq!(data, restored);
    }

    #[test]
    fn test_memory_delta() {
        let mut serializer = MemorySerializer::new();

        // Initial state
        let mut memory = vec![0u8; 2048];
        let _snapshot = serializer.create_snapshot(&memory).unwrap();

        // Modify some regions
        memory[100..200].fill(1);
        memory[1000..1100].fill(2);

        // Create delta
        let delta = serializer.create_delta(&memory, 1).unwrap();

        // Apply delta to original state
        let mut base = vec![0u8; 2048];
        delta.apply_to(&mut base).unwrap();

        // Should match modified memory
        assert_eq!(memory, base);
    }
}
