use crate::{
    Extern, error::InstantiationError, exports::Exports, imports::Imports,
    macros::backend::gen_rt_ty, module::Module, store::AsStoreMut,
};

/// A WebAssembly Instance is a stateful, executable
/// instance of a WebAssembly [`Module`].
///
/// Instance objects contain all the exported WebAssembly
/// functions, memories, tables and globals that allow
/// interacting with WebAssembly.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#module-instances>
#[derive(Clone, PartialEq, Eq)]
pub struct Instance {
    pub(crate) _inner: BackendInstance,
    pub(crate) module: Module,
    /// The exports for an instance.
    pub exports: Exports,
}

impl Instance {
    /// Creates a new `Instance` from a WebAssembly [`Module`] and a
    /// set of imports using [`Imports`] or the [`imports!`] macro helper.
    ///
    /// [`imports!`]: crate::imports!
    /// [`Imports!`]: crate::Imports!
    ///
    /// ```
    /// # use wasmer::{imports, Store, Module, Global, Value, Instance};
    /// # use wasmer::FunctionEnv;
    /// # fn main() -> anyhow::Result<()> {
    /// let mut store = Store::default();
    /// let env = FunctionEnv::new(&mut store, ());
    /// let module = Module::new(&store, "(module)")?;
    /// let imports = imports!{
    ///   "host" => {
    ///     "var" => Global::new(&mut store, Value::I32(2))
    ///   }
    /// };
    /// let instance = Instance::new(&mut store, &module, &imports)?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## Errors
    ///
    /// The function can return [`InstantiationError`]s.
    ///
    /// Those are, as defined by the spec:
    ///  * Link errors that happen when plugging the imports into the instance
    ///  * Runtime errors that happen when running the module `start` function.
    #[allow(clippy::result_large_err)]
    pub fn new(
        store: &mut impl AsStoreMut,
        module: &Module,
        imports: &Imports,
    ) -> Result<Self, InstantiationError> {
        let (_inner, exports) = match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::BackendStore::Sys(_) => {
                let (i, e) = crate::backend::sys::instance::Instance::new(store, module, imports)?;
                (crate::BackendInstance::Sys(i), e)
            }
            #[cfg(feature = "wamr")]
            crate::BackendStore::Wamr(_) => {
                let (i, e) = crate::backend::wamr::instance::Instance::new(store, module, imports)?;

                (crate::BackendInstance::Wamr(i), e)
            }
            #[cfg(feature = "wasmi")]
            crate::BackendStore::Wasmi(_) => {
                let (i, e) =
                    crate::backend::wasmi::instance::Instance::new(store, module, imports)?;

                (crate::BackendInstance::Wasmi(i), e)
            }
            #[cfg(feature = "v8")]
            crate::BackendStore::V8(_) => {
                let (i, e) = crate::backend::v8::instance::Instance::new(store, module, imports)?;
                (crate::BackendInstance::V8(i), e)
            }
            #[cfg(feature = "js")]
            crate::BackendStore::Js(_) => {
                let (i, e) = crate::backend::js::instance::Instance::new(store, module, imports)?;
                (crate::BackendInstance::Js(i), e)
            }
            #[cfg(feature = "jsc")]
            crate::BackendStore::Jsc(_) => {
                let (i, e) = crate::backend::jsc::instance::Instance::new(store, module, imports)?;
                (crate::BackendInstance::Jsc(i), e)
            }
        };

        Ok(Self {
            _inner,
            module: module.clone(),
            exports,
        })
    }

    /// Creates a new `Instance` from a WebAssembly [`Module`] and a
    /// vector of imports.
    ///
    /// ## Errors
    ///
    /// The function can return [`InstantiationError`]s.
    ///
    /// Those are, as defined by the spec:
    ///  * Link errors that happen when plugging the imports into the instance
    ///  * Runtime errors that happen when running the module `start` function.
    #[allow(clippy::result_large_err)]
    pub fn new_by_index(
        store: &mut impl AsStoreMut,
        module: &Module,
        externs: &[Extern],
    ) -> Result<Self, InstantiationError> {
        let (_inner, exports) = match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::BackendStore::Sys(_) => {
                let (i, e) =
                    crate::backend::sys::instance::Instance::new_by_index(store, module, externs)?;
                (crate::BackendInstance::Sys(i), e)
            }
            #[cfg(feature = "wamr")]
            crate::BackendStore::Wamr(_) => {
                let (i, e) =
                    crate::backend::wamr::instance::Instance::new_by_index(store, module, externs)?;

                (crate::BackendInstance::Wamr(i), e)
            }
            #[cfg(feature = "wasmi")]
            crate::BackendStore::Wasmi(_) => {
                let (i, e) = crate::backend::wasmi::instance::Instance::new_by_index(
                    store, module, externs,
                )?;

                (crate::BackendInstance::Wasmi(i), e)
            }
            #[cfg(feature = "v8")]
            crate::BackendStore::V8(_) => {
                let (i, e) =
                    crate::backend::v8::instance::Instance::new_by_index(store, module, externs)?;
                (crate::BackendInstance::V8(i), e)
            }
            #[cfg(feature = "js")]
            crate::BackendStore::Js(_) => {
                let (i, e) =
                    crate::backend::js::instance::Instance::new_by_index(store, module, externs)?;
                (crate::BackendInstance::Js(i), e)
            }
            #[cfg(feature = "jsc")]
            crate::BackendStore::Jsc(_) => {
                let (i, e) =
                    crate::backend::jsc::instance::Instance::new_by_index(store, module, externs)?;
                (crate::BackendInstance::Jsc(i), e)
            }
        };

        Ok(Self {
            _inner,
            module: module.clone(),
            exports,
        })
    }

    /// Gets the [`Module`] associated with this instance.
    pub fn module(&self) -> &Module {
        &self.module
    }

    /// Creates a checkpoint of the current instance state for live migration.
    ///
    /// This captures the complete execution state including:
    /// - Linear memory
    /// - Global variables
    /// - Call stack (interpreter frames)
    /// - Operand stack
    /// - Table state
    ///
    /// # Requirements
    ///
    /// - The `mvvm` feature must be enabled
    /// - The instance must be running on the WAMR backend
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The MVVM feature is not enabled
    /// - The backend is not WAMR
    /// - The current execution position is not checkpoint-safe
    ///
    /// # Example
    ///
    /// ```ignore
    /// use wasmer::{Instance, Store, Module};
    ///
    /// let checkpoint = instance.checkpoint(&store)?;
    /// let serialized = checkpoint.serialize()?;
    /// // Transfer serialized data to target machine...
    /// ```
    #[cfg(feature = "mvvm")]
    pub fn checkpoint(
        &self,
        store: &impl crate::AsStoreRef,
    ) -> Result<crate::wamr::migration::MvvmCheckpointData, crate::wamr::migration::MigrationError>
    {
        match &self._inner {
            #[cfg(feature = "wamr")]
            crate::BackendInstance::Wamr(instance) => instance.checkpoint(store),
            _ => Err(crate::wamr::migration::MigrationError::MvvmNotEnabled),
        }
    }

    /// Restores instance state from a checkpoint.
    ///
    /// This restores the complete execution state from a previously created checkpoint,
    /// allowing the instance to continue execution from where it left off.
    ///
    /// # Requirements
    ///
    /// - The `mvvm` feature must be enabled
    /// - The instance must be running on the WAMR backend
    /// - The checkpoint must have been created from a compatible module
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The MVVM feature is not enabled
    /// - The backend is not WAMR
    /// - The checkpoint is incompatible or corrupted
    /// - The module hash doesn't match
    ///
    /// # Example
    ///
    /// ```ignore
    /// use wasmer::{Instance, Store, Module};
    /// use wasmer::wamr::migration::MvvmCheckpointData;
    ///
    /// let checkpoint = MvvmCheckpointData::deserialize(&checkpoint_bytes)?;
    /// instance.restore_from_checkpoint(&checkpoint)?;
    /// // Continue execution...
    /// ```
    #[cfg(feature = "mvvm")]
    pub fn restore_from_checkpoint(
        &mut self,
        checkpoint: &crate::wamr::migration::MvvmCheckpointData,
    ) -> Result<(), crate::wamr::migration::MigrationError> {
        match &mut self._inner {
            #[cfg(feature = "wamr")]
            crate::BackendInstance::Wamr(instance) => instance.restore_from_checkpoint(checkpoint),
            _ => Err(crate::wamr::migration::MigrationError::MvvmNotEnabled),
        }
    }

    /// Returns true if a checkpoint can be taken at the current execution position.
    ///
    /// Checkpoints can only be taken at safe points during execution, typically
    /// at function boundaries or between instructions when no host calls are active.
    ///
    /// # Requirements
    ///
    /// - The `mvvm` feature must be enabled
    /// - The instance must be running on the WAMR backend
    ///
    /// # Returns
    ///
    /// - `true` if a checkpoint can be safely taken
    /// - `false` if the backend is not WAMR or if the current position is not checkpoint-safe
    #[cfg(feature = "mvvm")]
    pub fn is_checkpoint_safe(&self) -> bool {
        match &self._inner {
            #[cfg(feature = "wamr")]
            crate::BackendInstance::Wamr(instance) => instance.is_checkpoint_safe(),
            _ => false,
        }
    }

    /// Returns whether this instance supports MVVM checkpoint/restore.
    ///
    /// This is true only when:
    /// - The `mvvm` feature is enabled
    /// - The instance is running on the WAMR backend
    #[cfg(feature = "mvvm")]
    pub fn supports_checkpoint(&self) -> bool {
        matches!(&self._inner, crate::BackendInstance::Wamr(_))
    }
}

impl std::fmt::Debug for Instance {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("Instance")
            .field("exports", &self.exports)
            .finish()
    }
}

/// An enumeration of all the possible instances kind supported by the runtimes.
gen_rt_ty!(Instance @derives Clone, PartialEq, Eq);
