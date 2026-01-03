//! Data types, functions and traits for `wamr`'s `Instance` implementation.
use std::sync::Arc;

use crate::entities::external::VMExternToExtern;
use crate::{
    AsStoreMut, AsStoreRef, Exports, Extern, Imports, InstantiationError, Module,
    backend::wamr::bindings::*, vm::VMExtern, wamr::error::Trap,
};

#[cfg(feature = "mvvm")]
use crate::backend::wamr::migration::{
    InstructionPointer, MigrationError, MvvmCheckpointData, MvvmCheckpointable, TargetArchitecture,
};
#[cfg(feature = "mvvm")]
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(PartialEq, Eq)]
pub(crate) struct InstanceHandle(pub(crate) *mut wasm_instance_t);

unsafe impl Send for InstanceHandle {}
unsafe impl Sync for InstanceHandle {}

impl InstanceHandle {
    #[allow(clippy::result_large_err)]
    fn new(
        store: *mut wasm_store_t,
        module: *mut wasm_module_t,
        mut externs: Vec<VMExtern>,
    ) -> Result<Self, InstantiationError> {
        // Check if the thread env was already initialised.
        //unsafe {
        //    if !wasm_runtime_thread_env_inited() {
        //        if !wasm_runtime_init_thread_env() {
        //            panic!("Failed to initialize the thread environment!");
        //        }
        //    }
        //}

        let mut trap: *mut wasm_trap_t = std::ptr::null_mut() as _;
        let externs: Vec<_> = externs.into_iter().map(|v| v.into_wamr()).collect();

        let instance = unsafe {
            let mut imports = unsafe {
                let mut vec = Default::default();
                wasm_extern_vec_new(&mut vec, externs.len(), externs.as_ptr());
                vec
            };

            std::mem::forget(externs);

            wasm_instance_new(store, module, &imports, &mut trap)
        };

        if instance.is_null() {
            let trap = Trap::from(trap);
            return Err(InstantiationError::Start(trap.into()));
        }

        Ok(Self(instance))
    }

    fn get_exports(&self, mut store: &mut impl AsStoreMut, module: &Module) -> Exports {
        let mut c_api_externs = unsafe {
            let mut vec = Default::default();
            wasm_instance_exports(self.0, &mut vec);
            vec
        };

        let c_api_externs: Vec<*mut wasm_extern_t> = if c_api_externs.data.is_null()
            || !c_api_externs.data.is_aligned()
            || c_api_externs.size == 0
        {
            vec![]
        } else {
            unsafe { std::slice::from_raw_parts(c_api_externs.data, c_api_externs.size) }.to_vec()
        };
        let mut exports = unsafe {
            let mut vec = Default::default();
            wasm_module_exports(module.as_wamr().handle.inner, &mut vec);
            vec
        };

        let c_api_exports: Vec<*mut wasm_exporttype_t> =
            if exports.data.is_null() || !exports.data.is_aligned() || exports.size == 0 {
                vec![]
            } else {
                unsafe { std::slice::from_raw_parts(exports.data, exports.size) }.to_vec()
            };

        // We need to use the order of the exports from the polyfill info to get the right
        // one, i.e. the from the declaration.
        let module_exports = module.exports().collect::<Vec<_>>();

        let c_api_exports: Exports = c_api_exports
            .into_iter()
            .zip(c_api_externs)
            .map(|(export, ext)| unsafe {
                let name = wasm_exporttype_name(export);
                let name = std::slice::from_raw_parts((*name).data as *const u8, (*name).size);
                let mut name = name.to_vec();
                // Remove the '\0' at the end of the NULL-terminated string.
                name.pop();
                let name = String::from_utf8(name.to_vec()).unwrap_or_default();
                let ext = ext.to_extern(&mut store);
                (name, ext)
            })
            .collect();

        let mut exports = Exports::new();

        for e in module_exports {
            let ext: Extern = c_api_exports
                .get::<Extern>(e.name())
                .unwrap_or_else(|_| panic!("c_api_exports: {c_api_exports:?} (name: {})", e.name()))
                .clone();
            exports.insert(e.name().to_string(), ext);
        }

        exports
    }
}
impl Drop for InstanceHandle {
    fn drop(&mut self) {
        unsafe { wasm_instance_delete(self.0) }
    }
}

#[derive(Clone, PartialEq, Eq)]
/// A WebAssembly `instance` in `wamr`.
pub struct Instance {
    pub(crate) handle: Arc<InstanceHandle>,
}

impl Instance {
    #[allow(clippy::result_large_err)]
    pub(crate) fn new(
        store: &mut impl AsStoreMut,
        module: &Module,
        imports: &Imports,
    ) -> Result<(Self, Exports), InstantiationError> {
        let externs = module
            .imports()
            .map(|import_ty| {
                imports
                    .get_export(import_ty.module(), import_ty.name())
                    .expect("Extern not found")
            })
            .collect::<Vec<_>>();

        _ = store;
        // Hacky: we need to tie a *module* to a store before instantiating it..
        let wamr_module = module.as_wamr();
        let mut store_from_module = wamr_module.handle.store.lock().unwrap();
        let mut store = store_from_module.as_store_mut();

        Self::new_by_index(&mut store, module, &externs)
    }

    #[allow(clippy::result_large_err)]
    pub(crate) fn new_by_index(
        store: &mut impl AsStoreMut,
        module: &Module,
        externs: &[Extern],
    ) -> Result<(Self, Exports), InstantiationError> {
        let store_ref = store.as_store_ref();
        let externs: Vec<VMExtern> = externs
            .iter()
            .map(|extern_| extern_.to_vm_extern())
            .collect::<Vec<_>>();

        let instance = InstanceHandle::new(
            store_ref.inner.store.as_wamr().inner,
            module.as_wamr().handle.inner,
            externs,
        )?;
        let exports = instance.get_exports(store, module);

        Ok((
            Self {
                handle: Arc::new(instance),
            },
            exports,
        ))
    }
}

impl crate::BackendInstance {
    /// Consume [`self`] into a [`crate::backend::wamr::instance::Instance`].
    pub(crate) fn into_wamr(self) -> crate::backend::wamr::instance::Instance {
        match self {
            Self::Wamr(s) => s,
            _ => panic!("Not a `wamr` instance"),
        }
    }
}

// MVVM checkpoint/restore implementation
#[cfg(feature = "mvvm")]
impl Instance {
    /// Creates a checkpoint of the current instance state.
    ///
    /// This captures the complete execution state including:
    /// - Linear memory
    /// - Global variables
    /// - Call stack (interpreter frames)
    /// - Operand stack
    /// - Table state
    ///
    /// # Errors
    ///
    /// Returns an error if the current execution position is not checkpoint-safe.
    pub fn checkpoint(&self, store: &impl AsStoreRef) -> Result<MvvmCheckpointData, MigrationError> {
        use crate::backend::wamr::mvvm_bindings::*;

        // Get the raw instance pointer - cast to MVVM bindings type
        // Both wasm_instance_t types are structurally identical
        let instance_ptr = self.handle.0 as *mut wasm_instance_t;

        // Create checkpoint context
        let ctx = unsafe { mvvm_checkpoint_context_new(instance_ptr) };
        if ctx.is_null() {
            return Err(MigrationError::CheckpointFailed(
                "Failed to create checkpoint context".to_string(),
            ));
        }

        // Check if checkpoint is safe at current position
        let is_safe = unsafe { mvvm_is_checkpoint_safe(ctx) };
        if !is_safe {
            unsafe { mvvm_checkpoint_context_delete(ctx) };
            return Err(MigrationError::NotCheckpointSafe);
        }

        // Create the checkpoint
        let mut checkpoint_data = mvvm_checkpoint_data_t {
            version: 0,
            data_size: 0,
            data: std::ptr::null_mut(),
        };

        let result = unsafe { mvvm_create_checkpoint(ctx, &mut checkpoint_data) };
        if result != mvvm_result_t::MVVM_OK {
            unsafe { mvvm_checkpoint_context_delete(ctx) };
            return Err(MigrationError::CheckpointFailed(format!(
                "MVVM checkpoint failed with code: {:?}",
                result
            )));
        }

        // Get module hash for validation
        let mut module_hash = [0u8; 32];
        let hash_result = unsafe { mvvm_instance_get_module_hash(instance_ptr, &mut module_hash) };
        if hash_result != mvvm_result_t::MVVM_OK {
            // Use empty hash if not available
            module_hash = [0u8; 32];
        }

        // Copy checkpoint data to Rust-owned memory
        let data = if !checkpoint_data.data.is_null() && checkpoint_data.data_size > 0 {
            let slice = unsafe {
                std::slice::from_raw_parts(checkpoint_data.data, checkpoint_data.data_size)
            };
            slice.to_vec()
        } else {
            Vec::new()
        };

        // Free MVVM-allocated memory
        unsafe {
            mvvm_checkpoint_data_delete(&mut checkpoint_data);
            mvvm_checkpoint_context_delete(ctx);
        }

        // Parse the checkpoint data into our structured format
        // For now, we'll store the raw MVVM checkpoint data in the memory field
        // In a full implementation, we'd parse the MVVM format into our structured fields
        Ok(MvvmCheckpointData::new(
            TargetArchitecture::current(),
            module_hash,
            data.clone(), // Memory (raw MVVM checkpoint for now)
            Vec::new(),   // Globals (parsed from MVVM data)
            Vec::new(),   // Call stack
            Vec::new(),   // Data stack
            InstructionPointer::new(0, 0),
            Vec::new(), // Tables
            None,       // WASI state
        ))
    }

    /// Restores instance state from a checkpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the checkpoint is incompatible or corrupted.
    pub fn restore_from_checkpoint(
        &mut self,
        checkpoint: &MvvmCheckpointData,
    ) -> Result<(), MigrationError> {
        use crate::backend::wamr::mvvm_bindings::*;

        // Cast to MVVM bindings type (structurally identical)
        let instance_ptr = self.handle.0 as *mut wasm_instance_t;

        // Create checkpoint context
        let ctx = unsafe { mvvm_checkpoint_context_new(instance_ptr) };
        if ctx.is_null() {
            return Err(MigrationError::RestoreFailed(
                "Failed to create checkpoint context for restore".to_string(),
            ));
        }

        // Prepare checkpoint data for MVVM
        let checkpoint_data = mvvm_checkpoint_data_t {
            version: checkpoint.version,
            data_size: checkpoint.memory.len(),
            data: checkpoint.memory.as_ptr() as *mut u8,
        };

        // Restore the checkpoint
        let result = unsafe { mvvm_restore_checkpoint(ctx, &checkpoint_data) };

        unsafe { mvvm_checkpoint_context_delete(ctx) };

        if result != mvvm_result_t::MVVM_OK {
            return Err(MigrationError::RestoreFailed(format!(
                "MVVM restore failed with code: {:?}",
                result
            )));
        }

        Ok(())
    }

    /// Returns true if checkpoint can be taken at the current execution position.
    pub fn is_checkpoint_safe(&self) -> bool {
        use crate::backend::wamr::mvvm_bindings::*;

        // Cast to MVVM bindings type (structurally identical)
        let instance_ptr = self.handle.0 as *mut wasm_instance_t;
        let ctx = unsafe { mvvm_checkpoint_context_new(instance_ptr) };
        if ctx.is_null() {
            return false;
        }

        let is_safe = unsafe { mvvm_is_checkpoint_safe(ctx) };
        unsafe { mvvm_checkpoint_context_delete(ctx) };

        is_safe
    }
}

#[cfg(feature = "mvvm")]
impl MvvmCheckpointable for Instance {
    fn checkpoint(&self) -> Result<MvvmCheckpointData, MigrationError> {
        // Note: This implementation doesn't have access to store
        // For full functionality, use the method that takes store parameter
        Err(MigrationError::CheckpointFailed(
            "Use checkpoint() method with store parameter".to_string(),
        ))
    }

    fn restore(&mut self, checkpoint: &MvvmCheckpointData) -> Result<(), MigrationError> {
        self.restore_from_checkpoint(checkpoint)
    }

    fn is_checkpoint_safe(&self) -> bool {
        Instance::is_checkpoint_safe(self)
    }
}
