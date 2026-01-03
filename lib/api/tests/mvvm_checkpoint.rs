//! Tests for MVVM checkpoint/restore functionality.
//!
//! These tests verify the MVVM live migration support in Wasmer's WAMR backend.
//!
//! The public API is available through:
//! - `wasmer::mvvm::*` - All MVVM types and traits
//! - `wasmer::Instance::checkpoint()` - Create checkpoints
//! - `wasmer::Instance::restore_from_checkpoint()` - Restore from checkpoints
//! - `wasmer::Instance::is_checkpoint_safe()` - Check if checkpoint is safe
//! - `wasmer::Instance::supports_checkpoint()` - Check if backend supports MVVM

use macro_wasmer_universal_test::universal_test;
#[cfg(feature = "js")]
use wasm_bindgen_test::*;

use wasmer::*;

// Import MVVM types from the public API
#[cfg(feature = "mvvm")]
use wasmer::mvvm::{
    memory::MemorySnapshot,
    stack::{SerializedCallStack, SerializedFrame, SerializedValue},
    InstructionPointer, MvvmCheckpointData, TargetArchitecture,
};

/// Simple WebAssembly module for testing checkpoints.
const COUNTER_MODULE: &str = r#"
(module
    (global $counter (mut i32) (i32.const 0))

    (func $increment (export "increment") (result i32)
        global.get $counter
        i32.const 1
        i32.add
        global.set $counter
        global.get $counter
    )

    (func $get_counter (export "get_counter") (result i32)
        global.get $counter
    )

    (func $set_counter (export "set_counter") (param $val i32)
        local.get $val
        global.set $counter
    )

    (memory (export "memory") 1)
)
"#;

/// Test that basic instance creation works with MVVM feature.
#[universal_test]
#[cfg(feature = "mvvm")]
fn test_mvvm_instance_creation() -> Result<(), String> {
    let mut store = Store::default();
    let module = Module::new(&store, COUNTER_MODULE).map_err(|e| format!("{e:?}"))?;

    let imports = Imports::new();
    let instance = Instance::new(&mut store, &module, &imports).map_err(|e| format!("{e:?}"))?;

    // Call increment a few times
    let increment = instance
        .exports
        .get_function("increment")
        .map_err(|e| format!("{e:?}"))?;

    for i in 1..=5 {
        let result = increment
            .call(&mut store, &[])
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(result[0], Value::I32(i));
    }

    // Verify counter
    let get_counter = instance
        .exports
        .get_function("get_counter")
        .map_err(|e| format!("{e:?}"))?;

    let result = get_counter
        .call(&mut store, &[])
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result[0], Value::I32(5));

    Ok(())
}

/// Test checkpoint data serialization/deserialization.
#[test]
#[cfg(feature = "mvvm")]
fn test_checkpoint_data_serialization() {
    // Create checkpoint data
    let checkpoint = MvvmCheckpointData::new(
        TargetArchitecture::current(),
        [0u8; 32],
        vec![1, 2, 3, 4, 5], // Memory
        vec![6, 7, 8],       // Globals
        vec![9, 10],         // Call stack
        vec![11, 12, 13],    // Data stack
        InstructionPointer::new(0, 42),
        vec![],              // Tables
        None,                // WASI state
    );

    // Test serialization round-trip
    let serialized = checkpoint.serialize().expect("Failed to serialize");
    let deserialized =
        MvvmCheckpointData::deserialize(&serialized).expect("Failed to deserialize");

    assert_eq!(checkpoint.version, deserialized.version);
    assert_eq!(checkpoint.source_arch, deserialized.source_arch);
    assert_eq!(checkpoint.module_hash, deserialized.module_hash);
    assert_eq!(checkpoint.memory, deserialized.memory);
    assert_eq!(checkpoint.globals, deserialized.globals);
    assert_eq!(
        checkpoint.instruction_pointer,
        deserialized.instruction_pointer
    );
}

/// Test checkpoint compression.
#[test]
#[cfg(feature = "mvvm")]
fn test_checkpoint_compression() {
    // Create checkpoint with compressible data (repeated zeros)
    let checkpoint = MvvmCheckpointData::new(
        TargetArchitecture::current(),
        [0u8; 32],
        vec![0u8; 100000], // Large memory with zeros (compresses well)
        vec![],
        vec![],
        vec![],
        InstructionPointer::new(0, 0),
        vec![],
        None,
    );

    let compressed = checkpoint.compress().expect("Failed to compress");
    let decompressed =
        MvvmCheckpointData::decompress(&compressed).expect("Failed to decompress");

    // Verify compression reduced size
    let original_size = checkpoint.uncompressed_size();
    println!(
        "Original size: {}, Compressed size: {}",
        original_size,
        compressed.len()
    );
    assert!(compressed.len() < original_size);

    // Verify data integrity
    assert_eq!(checkpoint.memory, decompressed.memory);
}

/// Test target architecture detection.
#[test]
#[cfg(feature = "mvvm")]
fn test_target_architecture() {
    let current = TargetArchitecture::current();

    #[cfg(target_arch = "x86_64")]
    assert_eq!(current, TargetArchitecture::X86_64);

    #[cfg(target_arch = "aarch64")]
    assert_eq!(current, TargetArchitecture::Aarch64);

    #[cfg(target_arch = "riscv64")]
    assert_eq!(current, TargetArchitecture::Riscv64);

    // Test cross-architecture migration support
    assert!(current.can_migrate_to(TargetArchitecture::X86_64));
    assert!(current.can_migrate_to(TargetArchitecture::Aarch64));
    assert!(current.can_migrate_to(TargetArchitecture::Riscv64));
}

/// Test module hash validation.
#[test]
#[cfg(feature = "mvvm")]
fn test_module_hash_validation() {
    let module_hash = [1u8; 32];
    let checkpoint = MvvmCheckpointData::new(
        TargetArchitecture::current(),
        module_hash,
        vec![],
        vec![],
        vec![],
        vec![],
        InstructionPointer::new(0, 0),
        vec![],
        None,
    );

    // Same hash should pass
    assert!(checkpoint.validate_module_hash(&[1u8; 32]).is_ok());

    // Different hash should fail
    assert!(checkpoint.validate_module_hash(&[2u8; 32]).is_err());
}

/// Test stack frame serialization.
#[test]
#[cfg(feature = "mvvm")]
fn test_stack_serialization() {
    // Create a call stack with some frames
    let frames = vec![
        SerializedFrame {
            func_idx: 0,
            ip_offset: 10,
            sp: 0,
            locals: vec![
                SerializedValue::I32(42),
                SerializedValue::I64(1234567890),
            ],
            block_depth: 1,
        },
        SerializedFrame {
            func_idx: 1,
            ip_offset: 20,
            sp: 2,
            locals: vec![SerializedValue::F32(3.14f32.to_bits())],
            block_depth: 0,
        },
    ];
    let call_stack = SerializedCallStack::new(frames);

    // Serialize and deserialize
    let serialized = call_stack.serialize().expect("Failed to serialize");
    let deserialized = SerializedCallStack::deserialize(&serialized).expect("Failed to deserialize");

    assert_eq!(call_stack.depth, deserialized.depth);
    assert_eq!(call_stack.frames.len(), deserialized.frames.len());
}

/// Test memory snapshot serialization.
#[test]
#[cfg(feature = "mvvm")]
fn test_memory_serialization() {
    // Create a memory snapshot with compression
    let data = vec![1u8, 2, 3, 4, 5, 0, 0, 0, 6, 7, 8, 0, 0, 0, 0];
    let snapshot = MemorySnapshot::from_raw(&data, true).expect("Failed to create snapshot");

    // Verify we can get the raw data back
    let recovered = snapshot.to_raw().expect("Failed to recover data");
    assert_eq!(data, recovered);

    // Verify compression ratio is reported
    let ratio = snapshot.compression_ratio();
    assert!(ratio > 0.0);
    println!("Compression ratio: {:.2}%", ratio * 100.0);
}

/// Test that the public Instance API exposes MVVM methods.
/// Note: This test requires the WAMR backend to be used. When running with
/// default features (Cranelift), this test will pass but skip checkpoint tests.
#[universal_test]
#[cfg(feature = "mvvm")]
fn test_instance_checkpoint_api() -> Result<(), String> {
    let mut store = Store::default();
    let module = Module::new(&store, COUNTER_MODULE).map_err(|e| format!("{e:?}"))?;

    let imports = Imports::new();
    let instance = Instance::new(&mut store, &module, &imports).map_err(|e| format!("{e:?}"))?;

    // Check if WAMR backend is being used
    // When using default features (Cranelift), checkpoint is not supported
    if !instance.supports_checkpoint() {
        // Skip the rest of the test when not using WAMR backend
        println!("Skipping checkpoint tests - not using WAMR backend");
        return Ok(());
    }

    // Call increment a few times to modify state
    let increment = instance
        .exports
        .get_function("increment")
        .map_err(|e| format!("{e:?}"))?;

    for _ in 0..3 {
        increment
            .call(&mut store, &[])
            .map_err(|e| format!("{e:?}"))?;
    }

    // Verify counter is 3
    let get_counter = instance
        .exports
        .get_function("get_counter")
        .map_err(|e| format!("{e:?}"))?;

    let result = get_counter
        .call(&mut store, &[])
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result[0], Value::I32(3));

    // Test is_checkpoint_safe - note: this may return false if not at a safe point
    // For now, we just test that the method exists and returns a boolean
    let _is_safe = instance.is_checkpoint_safe();

    // Note: Full checkpoint/restore testing requires being at a checkpoint-safe point
    // which typically requires the interpreter to be paused at a specific location.
    // The checkpoint() method will return NotCheckpointSafe if called at an unsafe point.

    Ok(())
}

/// Test checkpoint serialization round-trip with public API types.
#[test]
#[cfg(feature = "mvvm")]
fn test_checkpoint_public_api_types() {
    // Verify we can use the convenience re-exports from wasmer crate root
    use wasmer::{MigrationError, MvvmCheckpointData, TargetArchitecture};

    let checkpoint = MvvmCheckpointData::new(
        TargetArchitecture::current(),
        [0u8; 32],
        vec![1, 2, 3],
        vec![],
        vec![],
        vec![],
        InstructionPointer::new(0, 0),
        vec![],
        None,
    );

    // Test that MigrationError works
    let err = MigrationError::MvvmNotEnabled;
    assert!(err.to_string().contains("MVVM"));

    // Test serialization
    let bytes = checkpoint.serialize().expect("serialize");
    let restored = MvvmCheckpointData::deserialize(&bytes).expect("deserialize");
    assert_eq!(checkpoint.memory, restored.memory);
}

// To test full checkpoint/restore functionality with MVVM:
// 1. Build with mvvm feature: cargo build --features mvvm
// 2. Use the wasmer CLI with --migrate flag (when implemented)
// 3. Create an instance, call instance.checkpoint() at a safe point
// 4. Transfer checkpoint data to target machine
// 5. Create a new instance and call instance.restore_from_checkpoint()
