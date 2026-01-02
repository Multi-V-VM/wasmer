//! MVVM Migration commands for checkpoint/restore functionality.
//!
//! This module provides CLI commands for checkpointing and restoring
//! WebAssembly execution state using MVVM (Multi-V-VM) technology.

use crate::commands::CliCommand;

mod checkpoint;
mod restore;

pub use checkpoint::*;
pub use restore::*;

/// MVVM Migration commands for checkpoint/restore and live migration.
#[derive(clap::Subcommand, Debug)]
pub enum CmdMigrate {
    /// Create a checkpoint of a running WebAssembly instance
    Checkpoint(CmdMigrateCheckpoint),
    /// Restore a WebAssembly instance from a checkpoint
    Restore(CmdMigrateRestore),
}

impl CliCommand for CmdMigrate {
    type Output = ();

    fn run(self) -> Result<(), anyhow::Error> {
        match self {
            Self::Checkpoint(cmd) => cmd.run(),
            Self::Restore(cmd) => cmd.run(),
        }
    }
}
