//! Data types, functions and traits for the `wamr` backend.

pub(crate) mod bindings;
pub(crate) mod entities;
pub(crate) mod error;
pub(crate) mod utils;
pub(crate) mod vm;

#[cfg(feature = "mvvm")]
pub(crate) mod mvvm_bindings;
#[cfg(feature = "mvvm")]
pub mod migration;

pub use entities::{engine::Engine as Wamr, *};

#[cfg(feature = "mvvm")]
pub use migration::{
    CrossArchMigratable, MigrationError, MvvmCheckpointData, MvvmCheckpointable,
    TargetArchitecture,
};
