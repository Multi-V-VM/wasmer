//! Stack frame serialization for MVVM checkpoints.
//!
//! This module provides utilities for serializing and deserializing
//! WebAssembly interpreter call stacks and operand stacks.
//!
//! # Architecture
//!
//! WAMR uses an interpreter with the following stack structure:
//!
//! - **Call Stack**: Contains activation frames for each function call
//! - **Operand Stack**: Contains values being computed (locals, temporaries)
//!
//! Each call frame includes:
//! - Function index
//! - Instruction pointer offset
//! - Stack pointer
//! - Local variables
//!
//! # Portability
//!
//! Stack serialization is architecture-independent because:
//! 1. Function indices are module-relative
//! 2. IP offsets are bytecode-relative
//! 3. Values are serialized as their WebAssembly types (i32, i64, f32, f64, etc.)

use super::MigrationError;
use serde::{Deserialize, Serialize};

/// WebAssembly value types for stack serialization.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[repr(u8)]
pub enum WasmValueType {
    /// 32-bit integer
    I32 = 0,
    /// 64-bit integer
    I64 = 1,
    /// 32-bit float
    F32 = 2,
    /// 64-bit float
    F64 = 3,
    /// 128-bit SIMD vector
    V128 = 4,
    /// Function reference
    FuncRef = 5,
    /// External reference
    ExternRef = 6,
}

/// A serialized WebAssembly value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SerializedValue {
    /// 32-bit integer value
    I32(i32),
    /// 64-bit integer value
    I64(i64),
    /// 32-bit float stored as bits to preserve NaN payloads
    F32(u32),
    /// 64-bit float stored as bits to preserve NaN payloads
    F64(u64),
    /// 128-bit SIMD vector
    V128([u8; 16]),
    /// Function reference (function index or null)
    FuncRef(Option<u32>),
    /// External reference (extern ref index or null)
    ExternRef(Option<u32>),
}

impl SerializedValue {
    /// Returns the type of this value.
    pub fn value_type(&self) -> WasmValueType {
        match self {
            Self::I32(_) => WasmValueType::I32,
            Self::I64(_) => WasmValueType::I64,
            Self::F32(_) => WasmValueType::F32,
            Self::F64(_) => WasmValueType::F64,
            Self::V128(_) => WasmValueType::V128,
            Self::FuncRef(_) => WasmValueType::FuncRef,
            Self::ExternRef(_) => WasmValueType::ExternRef,
        }
    }
}

/// A serialized call frame from the interpreter stack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SerializedFrame {
    /// Index of the function in the module
    pub func_idx: u32,

    /// Offset of the current instruction within the function's bytecode
    pub ip_offset: u32,

    /// Stack pointer value (position in operand stack)
    pub sp: u32,

    /// Local variable values
    pub locals: Vec<SerializedValue>,

    /// Block depth (for branch targets)
    pub block_depth: u32,
}

impl SerializedFrame {
    /// Creates a new serialized frame.
    pub fn new(
        func_idx: u32,
        ip_offset: u32,
        sp: u32,
        locals: Vec<SerializedValue>,
        block_depth: u32,
    ) -> Self {
        Self {
            func_idx,
            ip_offset,
            sp,
            locals,
            block_depth,
        }
    }
}

/// Complete serialized call stack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedCallStack {
    /// Version for compatibility
    pub version: u32,

    /// Stack frames (bottom to top)
    pub frames: Vec<SerializedFrame>,

    /// Total frame count
    pub depth: u32,
}

impl SerializedCallStack {
    /// Current serialization version
    pub const VERSION: u32 = 1;

    /// Creates a new serialized call stack.
    pub fn new(frames: Vec<SerializedFrame>) -> Self {
        let depth = frames.len() as u32;
        Self {
            version: Self::VERSION,
            frames,
            depth,
        }
    }

    /// Serializes to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, MigrationError> {
        bincode::serde::encode_to_vec(self, bincode::config::standard())
            .map_err(|e| MigrationError::SerializationError(e.to_string()))
    }

    /// Deserializes from bytes.
    pub fn deserialize(bytes: &[u8]) -> Result<Self, MigrationError> {
        let (stack, _): (Self, _) = bincode::serde::decode_from_slice(bytes, bincode::config::standard())
            .map_err(|e| MigrationError::SerializationError(e.to_string()))?;

        if stack.version != Self::VERSION {
            return Err(MigrationError::InvalidVersion(stack.version));
        }

        Ok(stack)
    }

    /// Returns true if the call stack is empty.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Returns the topmost frame (current function).
    pub fn top(&self) -> Option<&SerializedFrame> {
        self.frames.last()
    }
}

/// Serialized operand/data stack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedDataStack {
    /// Version for compatibility
    pub version: u32,

    /// Stack values (bottom to top)
    pub values: Vec<SerializedValue>,

    /// Stack pointer
    pub sp: u32,
}

impl SerializedDataStack {
    /// Current serialization version
    pub const VERSION: u32 = 1;

    /// Creates a new serialized data stack.
    pub fn new(values: Vec<SerializedValue>) -> Self {
        let sp = values.len() as u32;
        Self {
            version: Self::VERSION,
            values,
            sp,
        }
    }

    /// Serializes to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, MigrationError> {
        bincode::serde::encode_to_vec(self, bincode::config::standard())
            .map_err(|e| MigrationError::SerializationError(e.to_string()))
    }

    /// Deserializes from bytes.
    pub fn deserialize(bytes: &[u8]) -> Result<Self, MigrationError> {
        let (stack, _): (Self, _) = bincode::serde::decode_from_slice(bytes, bincode::config::standard())
            .map_err(|e| MigrationError::SerializationError(e.to_string()))?;

        if stack.version != Self::VERSION {
            return Err(MigrationError::InvalidVersion(stack.version));
        }

        Ok(stack)
    }

    /// Returns true if the data stack is empty.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Returns the current stack depth.
    pub fn depth(&self) -> usize {
        self.values.len()
    }
}

/// Serialized global variable state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedGlobals {
    /// Global variable values
    pub values: Vec<SerializedValue>,
}

impl SerializedGlobals {
    /// Creates a new globals state.
    pub fn new(values: Vec<SerializedValue>) -> Self {
        Self { values }
    }

    /// Serializes to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, MigrationError> {
        bincode::serde::encode_to_vec(self, bincode::config::standard())
            .map_err(|e| MigrationError::SerializationError(e.to_string()))
    }

    /// Deserializes from bytes.
    pub fn deserialize(bytes: &[u8]) -> Result<Self, MigrationError> {
        let (globals, _): (Self, _) = bincode::serde::decode_from_slice(bytes, bincode::config::standard())
            .map_err(|e| MigrationError::SerializationError(e.to_string()))?;
        Ok(globals)
    }
}

/// Serialized table state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedTable {
    /// Table index in the module
    pub table_idx: u32,

    /// Element type
    pub element_type: WasmValueType,

    /// Table entries
    pub entries: Vec<Option<u32>>, // Function indices for funcref, or extern indices
}

impl SerializedTable {
    /// Creates a new table state.
    pub fn new(table_idx: u32, element_type: WasmValueType, entries: Vec<Option<u32>>) -> Self {
        Self {
            table_idx,
            element_type,
            entries,
        }
    }
}

/// Complete table state for all tables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedTables {
    /// All table states
    pub tables: Vec<SerializedTable>,
}

impl SerializedTables {
    /// Creates a new tables state.
    pub fn new(tables: Vec<SerializedTable>) -> Self {
        Self { tables }
    }

    /// Serializes to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, MigrationError> {
        bincode::serde::encode_to_vec(self, bincode::config::standard())
            .map_err(|e| MigrationError::SerializationError(e.to_string()))
    }

    /// Deserializes from bytes.
    pub fn deserialize(bytes: &[u8]) -> Result<Self, MigrationError> {
        let (tables, _): (Self, _) = bincode::serde::decode_from_slice(bytes, bincode::config::standard())
            .map_err(|e| MigrationError::SerializationError(e.to_string()))?;
        Ok(tables)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialized_value_types() {
        assert_eq!(SerializedValue::I32(42).value_type(), WasmValueType::I32);
        assert_eq!(SerializedValue::I64(42).value_type(), WasmValueType::I64);
        assert_eq!(SerializedValue::F32(0).value_type(), WasmValueType::F32);
        assert_eq!(SerializedValue::F64(0).value_type(), WasmValueType::F64);
    }

    #[test]
    fn test_call_stack_serialization() {
        let frame = SerializedFrame::new(
            0,
            100,
            5,
            vec![SerializedValue::I32(42), SerializedValue::I64(100)],
            1,
        );

        let stack = SerializedCallStack::new(vec![frame.clone()]);

        let bytes = stack.serialize().unwrap();
        let restored = SerializedCallStack::deserialize(&bytes).unwrap();

        assert_eq!(restored.frames.len(), 1);
        assert_eq!(restored.frames[0], frame);
    }

    #[test]
    fn test_data_stack_serialization() {
        let values = vec![
            SerializedValue::I32(1),
            SerializedValue::I32(2),
            SerializedValue::I64(3),
        ];

        let stack = SerializedDataStack::new(values.clone());

        let bytes = stack.serialize().unwrap();
        let restored = SerializedDataStack::deserialize(&bytes).unwrap();

        assert_eq!(restored.values, values);
        assert_eq!(restored.sp, 3);
    }

    #[test]
    fn test_globals_serialization() {
        let values = vec![
            SerializedValue::I32(100),
            SerializedValue::F64(3.14_f64.to_bits()),
        ];

        let globals = SerializedGlobals::new(values.clone());

        let bytes = globals.serialize().unwrap();
        let restored = SerializedGlobals::deserialize(&bytes).unwrap();

        assert_eq!(restored.values, values);
    }
}
