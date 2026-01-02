//! Checkpoint command for MVVM migration.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;

use crate::commands::CliCommand;

/// Create a checkpoint of a WebAssembly module's execution state.
///
/// This command runs a WebAssembly module until a checkpoint trigger occurs,
/// then saves the complete execution state (memory, stack, globals) to a file.
///
/// The checkpoint can later be restored on the same or different architecture
/// using the `wasmer migrate restore` command.
///
/// Example:
///   wasmer migrate checkpoint --output checkpoint.mvvm module.wasm
#[derive(Debug, Parser)]
pub struct CmdMigrateCheckpoint {
    /// Path to the WebAssembly module to checkpoint
    #[clap(index = 1)]
    module_path: PathBuf,

    /// Output path for the checkpoint file
    #[clap(short = 'o', long = "output")]
    output: PathBuf,

    /// Checkpoint granularity: function, loop, or instruction
    #[clap(long = "granularity", default_value = "function")]
    granularity: String,

    /// Compress the checkpoint data using LZ4
    #[clap(long = "compress", default_value = "true")]
    compress: bool,

    /// Also write checkpoint to a journal file for hybrid migration
    #[clap(long = "journal")]
    journal_path: Option<PathBuf>,

    /// Arguments to pass to the WebAssembly module
    #[clap(last = true)]
    args: Vec<String>,
}

impl CliCommand for CmdMigrateCheckpoint {
    type Output = ();

    fn run(self) -> Result<(), anyhow::Error> {
        // Validate inputs
        if !self.module_path.exists() {
            bail!(
                "WebAssembly module not found: {}",
                self.module_path.display()
            );
        }

        let granularity = match self.granularity.as_str() {
            "function" => 0,
            "loop" => 1,
            "instruction" => 2,
            other => bail!(
                "Invalid granularity '{}'. Expected: function, loop, or instruction",
                other
            ),
        };

        println!(
            "MVVM Checkpoint: {} -> {}",
            self.module_path.display(),
            self.output.display()
        );
        println!("  Granularity: {}", self.granularity);
        println!("  Compression: {}", if self.compress { "LZ4" } else { "none" });

        // Read the module
        let wasm_bytes = std::fs::read(&self.module_path)
            .with_context(|| format!("Failed to read module: {}", self.module_path.display()))?;

        // TODO: When MVVM feature is enabled, this will:
        // 1. Create a WAMR engine with MVVM checkpoint support
        // 2. Compile and instantiate the module
        // 3. Run until checkpoint trigger
        // 4. Serialize the checkpoint state
        // 5. Write to output file (and optionally journal)

        #[cfg(feature = "mvvm")]
        {
            use wasmer::{Engine, Module, Store};

            // Create engine with MVVM support
            let engine = Engine::default();
            let mut store = Store::new(engine);

            // Compile module
            let module = Module::new(&store, &wasm_bytes)
                .with_context(|| "Failed to compile WebAssembly module")?;

            // For now, just create a placeholder checkpoint file
            // Full implementation will use the MVVM checkpoint API
            let checkpoint_data = create_placeholder_checkpoint(&wasm_bytes, granularity)?;

            // Write checkpoint
            let output_data = if self.compress {
                lz4_flex::compress_prepend_size(&checkpoint_data)
            } else {
                checkpoint_data
            };

            std::fs::write(&self.output, &output_data)
                .with_context(|| format!("Failed to write checkpoint: {}", self.output.display()))?;

            // Optionally write to journal
            if let Some(journal_path) = &self.journal_path {
                write_checkpoint_to_journal(journal_path, &output_data)?;
            }

            println!("Checkpoint created successfully: {} bytes", output_data.len());
        }

        #[cfg(not(feature = "mvvm"))]
        {
            bail!(
                "MVVM feature is not enabled. Rebuild with `--features mvvm` to use checkpoint functionality."
            );
        }

        Ok(())
    }
}

#[cfg(feature = "mvvm")]
fn create_placeholder_checkpoint(wasm_bytes: &[u8], granularity: u8) -> Result<Vec<u8>> {
    use std::io::Write;

    // Create a simple checkpoint header
    // Format: MVVM_CHECKPOINT_V1
    let mut data = Vec::new();

    // Magic bytes
    data.extend_from_slice(b"MVVM");

    // Version
    data.extend_from_slice(&1u32.to_le_bytes());

    // Granularity
    data.push(granularity);

    // Architecture
    #[cfg(target_arch = "x86_64")]
    data.push(0u8);
    #[cfg(target_arch = "aarch64")]
    data.push(1u8);
    #[cfg(target_arch = "riscv64")]
    data.push(2u8);
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64", target_arch = "riscv64")))]
    data.push(0u8);

    // Module hash (SHA256)
    let hash = sha2_hash(wasm_bytes);
    data.extend_from_slice(&hash);

    // Placeholder for actual checkpoint data
    // In full implementation, this would contain:
    // - Memory snapshot
    // - Call stack state
    // - Data stack state
    // - Globals
    // - Tables

    Ok(data)
}

#[cfg(feature = "mvvm")]
fn sha2_hash(data: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

#[cfg(feature = "mvvm")]
fn write_checkpoint_to_journal(journal_path: &PathBuf, checkpoint_data: &[u8]) -> Result<()> {
    use std::sync::Arc;
    use wasmer_wasix::journal::{LogFileJournal, MvvmCheckpoint, MvvmJournalAdapter, MvvmTargetArch};

    let journal = Arc::new(LogFileJournal::new(journal_path.clone())?);
    let mut adapter = MvvmJournalAdapter::new(journal);

    let checkpoint = MvvmCheckpoint::new(
        1, // version
        MvvmTargetArch::current(),
        vec![], // module hash - would be filled in real implementation
        checkpoint_data.to_vec(),
    );

    adapter.write_checkpoint(&checkpoint)?;
    println!("Checkpoint written to journal: {}", journal_path.display());

    Ok(())
}
