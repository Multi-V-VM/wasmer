//! FFI bindings for MVVM (Multi-V-VM) checkpoint/restore functionality.
//!
//! This module provides the low-level C FFI bindings for MVVM, which extends
//! WAMR with checkpoint and restore capabilities for live migration.

#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]
#![allow(clippy::all)]

// Include the auto-generated bindings from build.rs
#[cfg(feature = "mvvm")]
include!(concat!(env!("OUT_DIR"), "/mvvm_bindings.rs"));

// Re-export commonly used types from the generated bindings
#[cfg(feature = "mvvm")]
pub use self::{
    wasm_byte_vec_t, wasm_engine_delete, wasm_engine_new, wasm_engine_t, wasm_extern_t,
    wasm_extern_vec_t, wasm_instance_delete, wasm_instance_exports, wasm_instance_new,
    wasm_instance_t, wasm_module_delete, wasm_module_new, wasm_module_t, wasm_store_delete,
    wasm_store_new, wasm_store_t,
};

/// Opaque type for MVVM checkpoint context.
/// This is used to manage the checkpoint/restore state.
#[repr(C)]
#[derive(Debug)]
pub struct mvvm_checkpoint_context_t {
    _private: [u8; 0],
}

/// Opaque type for MVVM execution environment.
/// Contains the interpreter state including call stack and data stack.
#[repr(C)]
#[derive(Debug)]
pub struct mvvm_exec_env_t {
    _private: [u8; 0],
}

/// Opaque type for MVVM serializer.
/// Used to serialize/deserialize checkpoint data.
#[repr(C)]
#[derive(Debug)]
pub struct mvvm_serializer_t {
    _private: [u8; 0],
}

/// Opaque type for MVVM call frame.
/// Represents a single frame in the interpreter call stack.
#[repr(C)]
#[derive(Debug)]
pub struct mvvm_frame_t {
    _private: [u8; 0],
}

/// MVVM checkpoint data structure.
/// Contains all state needed to restore execution.
#[repr(C)]
#[derive(Debug)]
pub struct mvvm_checkpoint_data_t {
    /// Version of the checkpoint format
    pub version: u32,
    /// Size of the serialized data
    pub data_size: usize,
    /// Pointer to serialized data
    pub data: *mut u8,
}

/// Result codes for MVVM operations.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum mvvm_result_t {
    /// Operation succeeded
    MVVM_OK = 0,
    /// Generic error
    MVVM_ERROR = 1,
    /// Invalid argument
    MVVM_INVALID_ARG = 2,
    /// Out of memory
    MVVM_OUT_OF_MEMORY = 3,
    /// Checkpoint not supported at current position
    MVVM_CHECKPOINT_NOT_SUPPORTED = 4,
    /// Restore failed - incompatible checkpoint
    MVVM_RESTORE_INCOMPATIBLE = 5,
    /// Module hash mismatch
    MVVM_MODULE_HASH_MISMATCH = 6,
}

// Extern "C" declarations for MVVM-specific functions.
// These are the additional functions provided by MVVM on top of WAMR.
#[cfg(feature = "mvvm")]
extern "C" {
    /// Create a new checkpoint context for the given instance.
    pub fn mvvm_checkpoint_context_new(
        instance: *mut wasm_instance_t,
    ) -> *mut mvvm_checkpoint_context_t;

    /// Destroy a checkpoint context.
    pub fn mvvm_checkpoint_context_delete(ctx: *mut mvvm_checkpoint_context_t);

    /// Create a checkpoint of the current execution state.
    /// Returns the serialized checkpoint data.
    pub fn mvvm_create_checkpoint(
        ctx: *mut mvvm_checkpoint_context_t,
        data: *mut mvvm_checkpoint_data_t,
    ) -> mvvm_result_t;

    /// Restore execution state from a checkpoint.
    pub fn mvvm_restore_checkpoint(
        ctx: *mut mvvm_checkpoint_context_t,
        data: *const mvvm_checkpoint_data_t,
    ) -> mvvm_result_t;

    /// Free checkpoint data allocated by mvvm_create_checkpoint.
    pub fn mvvm_checkpoint_data_delete(data: *mut mvvm_checkpoint_data_t);

    /// Get the execution environment from an instance.
    pub fn mvvm_instance_get_exec_env(instance: *mut wasm_instance_t) -> *mut mvvm_exec_env_t;

    /// Get the current call frame from the execution environment.
    pub fn mvvm_exec_env_get_cur_frame(exec_env: *mut mvvm_exec_env_t) -> *mut mvvm_frame_t;

    /// Get the function index of a call frame.
    pub fn mvvm_frame_get_func_idx(frame: *mut mvvm_frame_t) -> u32;

    /// Get the instruction pointer offset of a call frame.
    pub fn mvvm_frame_get_ip_offset(frame: *mut mvvm_frame_t) -> u32;

    /// Get the stack pointer of a call frame.
    pub fn mvvm_frame_get_sp(frame: *mut mvvm_frame_t) -> u32;

    /// Get the previous frame in the call stack.
    pub fn mvvm_frame_get_prev(frame: *mut mvvm_frame_t) -> *mut mvvm_frame_t;

    /// Get linear memory data pointer.
    pub fn mvvm_instance_get_memory_data(instance: *mut wasm_instance_t) -> *mut u8;

    /// Get linear memory size in bytes.
    pub fn mvvm_instance_get_memory_size(instance: *mut wasm_instance_t) -> usize;

    /// Get the module hash for validation.
    pub fn mvvm_instance_get_module_hash(
        instance: *mut wasm_instance_t,
        hash: *mut [u8; 32],
    ) -> mvvm_result_t;

    /// Set checkpoint granularity.
    /// 0 = function level, 1 = loop level, 2 = instruction level
    pub fn mvvm_set_checkpoint_granularity(ctx: *mut mvvm_checkpoint_context_t, level: u32);

    /// Check if the current execution position is checkpoint-safe.
    pub fn mvvm_is_checkpoint_safe(ctx: *mut mvvm_checkpoint_context_t) -> bool;
}

/// Safe wrapper to check if MVVM checkpoint feature is available.
#[cfg(feature = "mvvm")]
pub fn mvvm_is_available() -> bool {
    true
}

#[cfg(not(feature = "mvvm"))]
pub fn mvvm_is_available() -> bool {
    false
}
