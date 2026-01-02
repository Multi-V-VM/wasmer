//! Data types, functions and traits for `wamr` runtime's `Engine` implementation.
use crate::{
    BackendEngine,
    backend::wamr::bindings::{wasm_engine_delete, wasm_engine_new, wasm_engine_t},
};
use std::sync::Arc;
use wasmer_types::{Features, target::Target};

#[derive(Debug)]
pub(crate) struct CApiEngine {
    pub(crate) engine: *mut wasm_engine_t,
}

impl Default for CApiEngine {
    fn default() -> Self {
        let engine: *mut wasm_engine_t = unsafe { wasm_engine_new() };
        Self { engine }
    }
}

impl Drop for CApiEngine {
    fn drop(&mut self) {
        unsafe { wasm_engine_delete(self.engine) }
    }
}

/// MVVM checkpoint configuration.
///
/// Controls how checkpoints are taken and at what granularity.
#[cfg(feature = "mvvm")]
#[derive(Clone, Debug, Default)]
pub struct MvvmConfig {
    /// Enable checkpoint/restore support
    pub checkpoint_enabled: bool,

    /// Enable AOT compilation for optimized checkpoint (required for best performance)
    pub aot_checkpoint: bool,

    /// Checkpoint granularity level
    pub checkpoint_granularity: MvvmCheckpointGranularity,
}

/// Checkpoint granularity levels.
///
/// Finer granularity allows checkpoints at more locations but with higher overhead.
#[cfg(feature = "mvvm")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum MvvmCheckpointGranularity {
    /// Checkpoints only at function entry/exit (lowest overhead)
    #[default]
    Function = 0,
    /// Checkpoints at loop iteration boundaries
    Loop = 1,
    /// Checkpoints at any instruction (highest overhead, most flexibility)
    Instruction = 2,
}

#[cfg(feature = "mvvm")]
impl MvvmConfig {
    /// Creates a new MVVM config with checkpoint support enabled.
    pub fn enabled() -> Self {
        Self {
            checkpoint_enabled: true,
            aot_checkpoint: false,
            checkpoint_granularity: MvvmCheckpointGranularity::Function,
        }
    }

    /// Creates a new MVVM config with AOT checkpoint support.
    pub fn with_aot() -> Self {
        Self {
            checkpoint_enabled: true,
            aot_checkpoint: true,
            checkpoint_granularity: MvvmCheckpointGranularity::Function,
        }
    }

    /// Sets the checkpoint granularity.
    pub fn with_granularity(mut self, granularity: MvvmCheckpointGranularity) -> Self {
        self.checkpoint_granularity = granularity;
        self
    }
}

/// The engine for the Web Assembly Micro Runtime.
#[derive(Clone, Debug)]
pub struct Engine {
    pub(crate) inner: Arc<CApiEngine>,
    /// MVVM configuration (when feature is enabled)
    #[cfg(feature = "mvvm")]
    pub(crate) mvvm_config: Option<MvvmConfig>,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            inner: Arc::new(CApiEngine::default()),
            #[cfg(feature = "mvvm")]
            mvvm_config: None,
        }
    }
}

impl Engine {
    /// Create a new instance of the `wamr` engine.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new engine with MVVM checkpoint support.
    #[cfg(feature = "mvvm")]
    pub fn new_with_mvvm(config: MvvmConfig) -> Self {
        Self {
            inner: Arc::new(CApiEngine::default()),
            mvvm_config: Some(config),
        }
    }

    /// Returns the MVVM configuration if enabled.
    #[cfg(feature = "mvvm")]
    pub fn mvvm_config(&self) -> Option<&MvvmConfig> {
        self.mvvm_config.as_ref()
    }

    /// Returns true if MVVM checkpoint support is enabled.
    #[cfg(feature = "mvvm")]
    pub fn is_mvvm_enabled(&self) -> bool {
        self.mvvm_config
            .as_ref()
            .map_or(false, |c| c.checkpoint_enabled)
    }

    pub(crate) fn deterministic_id(&self) -> String {
        #[cfg(feature = "mvvm")]
        if self.is_mvvm_enabled() {
            return String::from("wamr-mvvm");
        }
        String::from("wamr")
    }

    /// Returns the WebAssembly features supported by the WAMR engine.
    pub fn supported_features() -> Features {
        // WAMR-specific features
        let mut features = Features::default();
        features.bulk_memory(true);
        features.reference_types(true);
        features.multi_value(true);
        features.simd(false);
        features.threads(false);
        features.exceptions(false);
        features
    }

    /// Returns the default features for the WAMR engine.
    pub fn default_features() -> Features {
        Self::supported_features()
    }
}

unsafe impl Send for Engine {}
unsafe impl Sync for Engine {}

/// Returns the default engine for the wamr engine
pub(crate) fn default_engine() -> Engine {
    Engine::default()
}

impl crate::Engine {
    /// Consume [`self`] into a [`crate::backend::wamr::engine::Engine`].
    pub fn into_wamr(self) -> crate::backend::wamr::engine::Engine {
        match self.be {
            BackendEngine::Wamr(s) => s,
            _ => panic!("Not a `wamr` engine!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::wamr::engine::Engine`].
    pub fn as_wamr(&self) -> &crate::backend::wamr::engine::Engine {
        match &self.be {
            BackendEngine::Wamr(s) => s,
            _ => panic!("Not a `wamr` engine!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::wamr::engine::Engine`].
    pub fn as_wamr_mut(&mut self) -> &mut crate::backend::wamr::engine::Engine {
        match &mut self.be {
            BackendEngine::Wamr(s) => s,
            _ => panic!("Not a `wamr` engine!"),
        }
    }

    /// Return true if [`self`] is an engine from the `wamr` runtime.
    pub fn is_wamr(&self) -> bool {
        matches!(self.be, BackendEngine::Wamr(_))
    }
}

impl From<Engine> for crate::Engine {
    fn from(value: Engine) -> Self {
        Self {
            be: BackendEngine::Wamr(value),
            id: Self::atomic_next_engine_id(),
        }
    }
}
