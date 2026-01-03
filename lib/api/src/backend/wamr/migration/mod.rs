//! MVVM Migration Infrastructure
//!
//! This module provides the core types and traits for live migration support
//! using MVVM (Multi-V-VM) checkpoint/restore capabilities.
//!
//! # Architecture
//!
//! The migration system is built on three core concepts:
//!
//! 1. **Checkpoint Data** - Serialized state of a WebAssembly instance
//! 2. **Checkpointable Trait** - Interface for types that can be checkpointed
//! 3. **Cross-Architecture Migration** - Support for migrating between different CPU architectures
//!
//! # Example
//!
//! ```ignore
//! use wasmer::wamr::migration::{MvvmCheckpointable, MvvmCheckpointData};
//!
//! // Create checkpoint
//! let checkpoint = instance.checkpoint()?;
//!
//! // Serialize for transfer
//! let bytes = checkpoint.serialize()?;
//!
//! // On target machine: restore
//! let restored = Instance::restore_from_checkpoint(&store, &module, &checkpoint)?;
//! ```

pub mod memory;
pub mod stack;

use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use thiserror::Error;

/// Target architecture for cross-platform migration.
///
/// MVVM supports migration between different CPU architectures because
/// WebAssembly pointers are offsets into linear memory, not absolute addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum TargetArchitecture {
    /// x86-64 / AMD64
    X86_64 = 0,
    /// ARM 64-bit (AArch64)
    Aarch64 = 1,
    /// RISC-V 64-bit
    Riscv64 = 2,
}

impl TargetArchitecture {
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
            compile_error!("Unsupported target architecture for MVVM migration")
        }
    }

    /// Returns true if migration from this architecture to the target is supported.
    #[must_use]
    pub fn can_migrate_to(&self, target: Self) -> bool {
        // MVVM supports cross-architecture migration for all supported architectures
        // because WebAssembly uses abstract stack machines and relative memory offsets
        matches!(
            (self, target),
            (Self::X86_64, _) | (Self::Aarch64, _) | (Self::Riscv64, _)
        )
    }
}

impl std::fmt::Display for TargetArchitecture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::X86_64 => write!(f, "x86_64"),
            Self::Aarch64 => write!(f, "aarch64"),
            Self::Riscv64 => write!(f, "riscv64"),
        }
    }
}

/// Instruction pointer state for checkpoint.
///
/// Identifies the exact position in WebAssembly execution where
/// the checkpoint was taken.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstructionPointer {
    /// Index of the function being executed
    pub function_index: u32,
    /// Offset within the function's bytecode
    pub instruction_offset: u32,
}

impl InstructionPointer {
    /// Creates a new instruction pointer.
    pub fn new(function_index: u32, instruction_offset: u32) -> Self {
        Self {
            function_index,
            instruction_offset,
        }
    }
}

/// Checkpoint granularity level.
///
/// Controls at what points during execution checkpoints can be taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[repr(u8)]
pub enum CheckpointGranularity {
    /// Checkpoints only at function boundaries
    #[default]
    Function = 0,
    /// Checkpoints at loop iteration boundaries
    Loop = 1,
    /// Checkpoints at any instruction (most expensive)
    Instruction = 2,
}

/// Complete checkpoint data for a WebAssembly instance.
///
/// This structure contains all state needed to restore execution
/// on any supported architecture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MvvmCheckpointData {
    /// Checkpoint format version for compatibility checking
    pub version: u32,

    /// Architecture where the checkpoint was taken
    pub source_arch: TargetArchitecture,

    /// SHA-256 hash of the WebAssembly module for validation
    pub module_hash: [u8; 32],

    /// Serialized linear memory (LZ4 compressed)
    pub memory: Vec<u8>,

    /// Serialized global variables
    pub globals: Vec<u8>,

    /// Serialized interpreter call stack
    pub call_stack: Vec<u8>,

    /// Serialized operand/data stack
    pub data_stack: Vec<u8>,

    /// Current instruction pointer
    pub instruction_pointer: InstructionPointer,

    /// Serialized table state
    pub tables: Vec<u8>,

    /// Optional WASI state (file descriptors, environment, etc.)
    /// This is populated when running under WASIX
    pub wasi_state: Option<Vec<u8>>,

    /// Timestamp when the checkpoint was taken
    pub timestamp: SystemTime,
}

impl MvvmCheckpointData {
    /// Current checkpoint format version
    pub const CURRENT_VERSION: u32 = 1;

    /// Creates a new checkpoint data structure.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source_arch: TargetArchitecture,
        module_hash: [u8; 32],
        memory: Vec<u8>,
        globals: Vec<u8>,
        call_stack: Vec<u8>,
        data_stack: Vec<u8>,
        instruction_pointer: InstructionPointer,
        tables: Vec<u8>,
        wasi_state: Option<Vec<u8>>,
    ) -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            source_arch,
            module_hash,
            memory,
            globals,
            call_stack,
            data_stack,
            instruction_pointer,
            tables,
            wasi_state,
            timestamp: SystemTime::now(),
        }
    }

    /// Serializes the checkpoint data to bytes using bincode.
    pub fn serialize(&self) -> Result<Vec<u8>, MigrationError> {
        bincode::serde::encode_to_vec(self, bincode::config::standard())
            .map_err(|e| MigrationError::SerializationError(e.to_string()))
    }

    /// Deserializes checkpoint data from bytes.
    pub fn deserialize(bytes: &[u8]) -> Result<Self, MigrationError> {
        let (data, _): (Self, _) = bincode::serde::decode_from_slice(bytes, bincode::config::standard())
            .map_err(|e| MigrationError::SerializationError(e.to_string()))?;
        Ok(data)
    }

    /// Compresses the checkpoint data for efficient transfer.
    pub fn compress(&self) -> Result<Vec<u8>, MigrationError> {
        let serialized = self.serialize()?;
        Ok(lz4_flex::compress_prepend_size(&serialized))
    }

    /// Decompresses and deserializes checkpoint data.
    pub fn decompress(compressed: &[u8]) -> Result<Self, MigrationError> {
        let decompressed = lz4_flex::decompress_size_prepended(compressed)
            .map_err(|e| MigrationError::SerializationError(e.to_string()))?;
        Self::deserialize(&decompressed)
    }

    /// Validates that this checkpoint is compatible with the given module hash.
    pub fn validate_module_hash(&self, expected_hash: &[u8; 32]) -> Result<(), MigrationError> {
        if &self.module_hash != expected_hash {
            return Err(MigrationError::ModuleHashMismatch {
                expected: *expected_hash,
                actual: self.module_hash,
            });
        }
        Ok(())
    }

    /// Returns the total uncompressed size of the checkpoint.
    pub fn uncompressed_size(&self) -> usize {
        self.memory.len()
            + self.globals.len()
            + self.call_stack.len()
            + self.data_stack.len()
            + self.tables.len()
            + self.wasi_state.as_ref().map_or(0, Vec::len)
    }
}

/// Error types for migration operations.
#[derive(Debug, Error)]
pub enum MigrationError {
    /// Failed to create checkpoint
    #[error("Checkpoint creation failed: {0}")]
    CheckpointFailed(String),

    /// Failed to restore from checkpoint
    #[error("Restore failed: {0}")]
    RestoreFailed(String),

    /// Source and target architectures are incompatible
    #[error("Incompatible architecture: from={from_arch}, to={to_arch}")]
    IncompatibleArchitecture {
        /// Source architecture
        from_arch: TargetArchitecture,
        /// Target architecture
        to_arch: TargetArchitecture,
    },

    /// Module hash doesn't match the checkpoint
    #[error("Module hash mismatch: expected {expected:?}, got {actual:?}")]
    ModuleHashMismatch {
        /// Expected module hash
        expected: [u8; 32],
        /// Actual module hash
        actual: [u8; 32],
    },

    /// Serialization or deserialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// I/O error during checkpoint save/load
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// The execution position is not checkpoint-safe
    #[error("Current execution position is not checkpoint-safe")]
    NotCheckpointSafe,

    /// MVVM feature is not enabled
    #[error("MVVM feature is not enabled")]
    MvvmNotEnabled,

    /// Invalid checkpoint version
    #[error("Invalid checkpoint version: {0}")]
    InvalidVersion(u32),
}

/// Trait for types that can be checkpointed and restored via MVVM.
pub trait MvvmCheckpointable {
    /// Creates a checkpoint of the current execution state.
    ///
    /// # Errors
    ///
    /// Returns an error if the checkpoint cannot be created, for example
    /// if the current execution position is not checkpoint-safe.
    fn checkpoint(&self) -> Result<MvvmCheckpointData, MigrationError>;

    /// Restores execution state from a checkpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the checkpoint is incompatible or corrupted.
    fn restore(&mut self, checkpoint: &MvvmCheckpointData) -> Result<(), MigrationError>;

    /// Returns true if a checkpoint can be taken at the current execution position.
    fn is_checkpoint_safe(&self) -> bool;
}

/// Trait for types that support cross-architecture migration.
pub trait CrossArchMigratable: MvvmCheckpointable {
    /// Checks if migration from the current architecture to the target is supported.
    fn can_migrate_to(&self, target: TargetArchitecture) -> bool;

    /// Transforms checkpoint data for the target architecture if needed.
    ///
    /// For most cases, no transformation is needed because WebAssembly uses
    /// abstract stack machines and relative memory offsets.
    fn transform_for_arch(
        checkpoint: MvvmCheckpointData,
        target: TargetArchitecture,
    ) -> Result<MvvmCheckpointData, MigrationError> {
        // Default implementation: no transformation needed
        // WebAssembly's abstract machine model makes cross-arch migration straightforward
        if !checkpoint.source_arch.can_migrate_to(target) {
            return Err(MigrationError::IncompatibleArchitecture {
                from_arch: checkpoint.source_arch,
                to_arch: target,
            });
        }
        Ok(checkpoint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target_architecture_current() {
        let current = TargetArchitecture::current();
        #[cfg(target_arch = "x86_64")]
        assert_eq!(current, TargetArchitecture::X86_64);
        #[cfg(target_arch = "aarch64")]
        assert_eq!(current, TargetArchitecture::Aarch64);
    }

    #[test]
    fn test_checkpoint_data_serialization() {
        let checkpoint = MvvmCheckpointData::new(
            TargetArchitecture::X86_64,
            [0u8; 32],
            vec![1, 2, 3, 4],
            vec![5, 6],
            vec![7, 8, 9],
            vec![10],
            InstructionPointer::new(0, 42),
            vec![],
            None,
        );

        let serialized = checkpoint.serialize().unwrap();
        let deserialized = MvvmCheckpointData::deserialize(&serialized).unwrap();

        assert_eq!(checkpoint.version, deserialized.version);
        assert_eq!(checkpoint.source_arch, deserialized.source_arch);
        assert_eq!(checkpoint.memory, deserialized.memory);
        assert_eq!(
            checkpoint.instruction_pointer,
            deserialized.instruction_pointer
        );
    }

    #[test]
    fn test_checkpoint_compression() {
        let checkpoint = MvvmCheckpointData::new(
            TargetArchitecture::X86_64,
            [0u8; 32],
            vec![0u8; 10000], // Compressible data
            vec![],
            vec![],
            vec![],
            InstructionPointer::new(0, 0),
            vec![],
            None,
        );

        let compressed = checkpoint.compress().unwrap();
        let decompressed = MvvmCheckpointData::decompress(&compressed).unwrap();

        // Compressed should be smaller than original
        assert!(compressed.len() < checkpoint.uncompressed_size());
        assert_eq!(checkpoint.memory, decompressed.memory);
    }

    #[test]
    fn test_module_hash_validation() {
        let checkpoint = MvvmCheckpointData::new(
            TargetArchitecture::X86_64,
            [1u8; 32],
            vec![],
            vec![],
            vec![],
            vec![],
            InstructionPointer::new(0, 0),
            vec![],
            None,
        );

        // Matching hash should succeed
        assert!(checkpoint.validate_module_hash(&[1u8; 32]).is_ok());

        // Mismatched hash should fail
        assert!(checkpoint.validate_module_hash(&[2u8; 32]).is_err());
    }
}
