//! MVVM Journal Adapter
//!
//! This module provides an adapter to bridge MVVM checkpoint/restore
//! with Wasmer's journal system for hybrid live migration support.
//!
//! # Architecture
//!
//! The adapter works as a bridge between two checkpoint systems:
//!
//! 1. **MVVM Checkpoints**: Low-level WebAssembly state (memory, stack, globals)
//! 2. **WASIX Journal**: High-level OS state (file descriptors, networking, etc.)
//!
//! For complete live migration, both systems work together:
//! - MVVM captures the WebAssembly execution state
//! - WASIX journal captures the OS-level state
//!
//! # Example
//!
//! ```ignore
//! use wasmer_wasix::journal::MvvmJournalAdapter;
//!
//! // Create adapter with existing journal
//! let adapter = MvvmJournalAdapter::new(journal);
//!
//! // Write an MVVM checkpoint to the journal
//! adapter.write_checkpoint(&mvvm_checkpoint_data)?;
//!
//! // Read checkpoint back
//! let checkpoint = adapter.read_latest_checkpoint()?;
//! ```

use std::borrow::Cow;
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::Result;
use wasmer_journal::{JournalEntry, ReadableJournal, WritableJournal};

/// Target architecture enumeration matching MVVM's architecture types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MvvmTargetArch {
    X86_64 = 0,
    Aarch64 = 1,
    Riscv64 = 2,
}

impl MvvmTargetArch {
    /// Returns the current host architecture.
    #[must_use]
    pub fn current() -> Self {
        #[cfg(target_arch = "x86_64")]
        {
            Self::X86_64
        }
        #[cfg(target_arch = "aarch64")]
        {
            Self::Aarch64
        }
        #[cfg(target_arch = "riscv64")]
        {
            Self::Riscv64
        }
        #[cfg(not(any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "riscv64"
        )))]
        {
            Self::X86_64 // Default fallback
        }
    }

    /// Converts to u8 for journal serialization.
    pub fn to_u8(self) -> u8 {
        self as u8
    }

    /// Creates from u8 value.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::X86_64),
            1 => Some(Self::Aarch64),
            2 => Some(Self::Riscv64),
            _ => None,
        }
    }
}

/// MVVM checkpoint data structure for journal integration.
#[derive(Debug, Clone)]
pub struct MvvmCheckpoint {
    /// Checkpoint format version
    pub version: u32,
    /// Source architecture
    pub source_arch: MvvmTargetArch,
    /// Module hash for validation
    pub module_hash: Vec<u8>,
    /// Compressed checkpoint data (LZ4)
    pub data: Vec<u8>,
    /// Timestamp when checkpoint was taken
    pub timestamp: SystemTime,
}

impl MvvmCheckpoint {
    /// Creates a new MVVM checkpoint.
    pub fn new(
        version: u32,
        source_arch: MvvmTargetArch,
        module_hash: Vec<u8>,
        data: Vec<u8>,
    ) -> Self {
        Self {
            version,
            source_arch,
            module_hash,
            data,
            timestamp: SystemTime::now(),
        }
    }

    /// Compresses checkpoint data using LZ4.
    pub fn compress_data(data: &[u8]) -> Vec<u8> {
        lz4_flex::compress_prepend_size(data)
    }

    /// Decompresses checkpoint data.
    pub fn decompress_data(compressed: &[u8]) -> Result<Vec<u8>> {
        lz4_flex::decompress_size_prepended(compressed)
            .map_err(|e| anyhow::anyhow!("LZ4 decompression failed: {}", e))
    }
}

/// Adapter to bridge MVVM checkpoints with the Wasmer journal system.
pub struct MvvmJournalAdapter<J: WritableJournal + Send + Sync> {
    /// The underlying journal
    journal: Arc<J>,
    /// Counter for checkpoint IDs
    next_checkpoint_id: u64,
}

impl<J: WritableJournal + Send + Sync> MvvmJournalAdapter<J> {
    /// Creates a new MVVM journal adapter.
    pub fn new(journal: Arc<J>) -> Self {
        Self {
            journal,
            next_checkpoint_id: 1,
        }
    }

    /// Writes an MVVM checkpoint to the journal.
    pub fn write_checkpoint(&mut self, checkpoint: &MvvmCheckpoint) -> Result<u64> {
        let checkpoint_id = self.next_checkpoint_id;
        self.next_checkpoint_id += 1;

        // Compress the checkpoint data
        let compressed_data = if checkpoint.data.len() > 1024 {
            MvvmCheckpoint::compress_data(&checkpoint.data)
        } else {
            checkpoint.data.clone()
        };

        // Write to journal
        self.journal.write(JournalEntry::MvvmCheckpointV1 {
            version: checkpoint.version,
            source_arch: checkpoint.source_arch.to_u8(),
            module_hash: checkpoint.module_hash.clone().into_boxed_slice(),
            checkpoint_data: Cow::Owned(compressed_data),
            when: checkpoint.timestamp,
        })?;

        Ok(checkpoint_id)
    }

    /// Writes an incremental checkpoint (delta from previous).
    pub fn write_incremental_checkpoint(
        &mut self,
        base_checkpoint_id: u64,
        memory_delta: &[u8],
        stack_delta: &[u8],
    ) -> Result<u64> {
        let checkpoint_id = self.next_checkpoint_id;
        self.next_checkpoint_id += 1;

        // Compress deltas
        let compressed_memory = MvvmCheckpoint::compress_data(memory_delta);
        let compressed_stack = MvvmCheckpoint::compress_data(stack_delta);

        self.journal
            .write(JournalEntry::MvvmIncrementalCheckpointV1 {
                base_checkpoint_id,
                memory_delta: Cow::Owned(compressed_memory),
                stack_delta: Cow::Owned(compressed_stack),
                when: SystemTime::now(),
            })?;

        Ok(checkpoint_id)
    }

    /// Writes a migration request to the journal.
    pub fn write_migration_request(
        &mut self,
        target_arch: MvvmTargetArch,
        target_endpoint: &str,
        priority: u8,
    ) -> Result<()> {
        self.journal.write(JournalEntry::MigrationRequestV1 {
            target_arch: target_arch.to_u8(),
            target_endpoint: Cow::Owned(target_endpoint.to_string()),
            priority,
            when: SystemTime::now(),
        })?;

        Ok(())
    }

    /// Returns the next checkpoint ID that will be used.
    pub fn next_checkpoint_id(&self) -> u64 {
        self.next_checkpoint_id
    }
}

/// Reader for MVVM checkpoints from a journal.
pub struct MvvmJournalReader<'a, J: ReadableJournal> {
    journal: &'a J,
}

impl<'a, J: ReadableJournal> MvvmJournalReader<'a, J> {
    /// Creates a new journal reader.
    pub fn new(journal: &'a J) -> Self {
        Self { journal }
    }

    /// Reads the latest MVVM checkpoint from the journal.
    pub fn read_latest_checkpoint(&self) -> Result<Option<MvvmCheckpoint>> {
        let mut latest_checkpoint: Option<MvvmCheckpoint> = None;

        while let Some(entry) = self.journal.read()? {
            if let JournalEntry::MvvmCheckpointV1 {
                version,
                source_arch,
                module_hash,
                checkpoint_data,
                when,
            } = entry.into_inner()
            {
                // Decompress the data
                let data = if checkpoint_data.len() > 4 {
                    MvvmCheckpoint::decompress_data(&checkpoint_data).unwrap_or_else(|_| checkpoint_data.to_vec())
                } else {
                    checkpoint_data.to_vec()
                };

                latest_checkpoint = Some(MvvmCheckpoint {
                    version,
                    source_arch: MvvmTargetArch::from_u8(source_arch).unwrap_or(MvvmTargetArch::X86_64),
                    module_hash: module_hash.to_vec(),
                    data,
                    timestamp: when,
                });
            }
        }

        Ok(latest_checkpoint)
    }

    /// Reads all MVVM checkpoints from the journal.
    pub fn read_all_checkpoints(&self) -> Result<Vec<MvvmCheckpoint>> {
        let mut checkpoints = Vec::new();

        while let Some(entry) = self.journal.read()? {
            if let JournalEntry::MvvmCheckpointV1 {
                version,
                source_arch,
                module_hash,
                checkpoint_data,
                when,
            } = entry.into_inner()
            {
                let data = MvvmCheckpoint::decompress_data(&checkpoint_data)
                    .unwrap_or_else(|_| checkpoint_data.to_vec());

                checkpoints.push(MvvmCheckpoint {
                    version,
                    source_arch: MvvmTargetArch::from_u8(source_arch).unwrap_or(MvvmTargetArch::X86_64),
                    module_hash: module_hash.to_vec(),
                    data,
                    timestamp: when,
                });
            }
        }

        Ok(checkpoints)
    }

    /// Checks if there are any migration requests in the journal.
    pub fn has_migration_request(&self) -> Result<bool> {
        while let Some(entry) = self.journal.read()? {
            if matches!(entry.into_inner(), JournalEntry::MigrationRequestV1 { .. }) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target_arch_conversion() {
        assert_eq!(MvvmTargetArch::X86_64.to_u8(), 0);
        assert_eq!(MvvmTargetArch::Aarch64.to_u8(), 1);
        assert_eq!(MvvmTargetArch::Riscv64.to_u8(), 2);

        assert_eq!(MvvmTargetArch::from_u8(0), Some(MvvmTargetArch::X86_64));
        assert_eq!(MvvmTargetArch::from_u8(1), Some(MvvmTargetArch::Aarch64));
        assert_eq!(MvvmTargetArch::from_u8(2), Some(MvvmTargetArch::Riscv64));
        assert_eq!(MvvmTargetArch::from_u8(3), None);
    }

    #[test]
    fn test_checkpoint_compression() {
        let data = vec![0u8; 10000];
        let compressed = MvvmCheckpoint::compress_data(&data);

        // Compressed should be smaller
        assert!(compressed.len() < data.len());

        // Should round-trip
        let decompressed = MvvmCheckpoint::decompress_data(&compressed).unwrap();
        assert_eq!(data, decompressed);
    }
}
