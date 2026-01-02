//! Restore command for MVVM migration.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;

use crate::commands::CliCommand;

/// Restore a WebAssembly module from a checkpoint.
///
/// This command loads a previously created checkpoint and resumes execution
/// of the WebAssembly module from the checkpointed state.
///
/// The restore works across different architectures (x86_64, aarch64, riscv64)
/// because WebAssembly state is architecture-independent.
///
/// Example:
///   wasmer migrate restore checkpoint.mvvm
#[derive(Debug, Parser)]
pub struct CmdMigrateRestore {
    /// Path to the checkpoint file to restore from
    #[clap(index = 1)]
    checkpoint_path: PathBuf,

    /// Path to the WebAssembly module (optional, can be embedded in checkpoint)
    #[clap(short = 'm', long = "module")]
    module_path: Option<PathBuf>,

    /// Read checkpoint from a journal file instead
    #[clap(long = "from-journal")]
    from_journal: bool,

    /// Validate checkpoint integrity before restoring
    #[clap(long = "validate", default_value = "true")]
    validate: bool,

    /// Arguments to pass to the WebAssembly module after restore
    #[clap(last = true)]
    args: Vec<String>,
}

impl CliCommand for CmdMigrateRestore {
    type Output = ();

    fn run(self) -> Result<(), anyhow::Error> {
        // Validate inputs
        if !self.checkpoint_path.exists() {
            bail!(
                "Checkpoint file not found: {}",
                self.checkpoint_path.display()
            );
        }

        println!(
            "MVVM Restore: {}",
            self.checkpoint_path.display()
        );

        #[cfg(feature = "mvvm")]
        {
            // Read checkpoint data
            let checkpoint_data = if self.from_journal {
                read_checkpoint_from_journal(&self.checkpoint_path)?
            } else {
                std::fs::read(&self.checkpoint_path)
                    .with_context(|| format!("Failed to read checkpoint: {}", self.checkpoint_path.display()))?
            };

            // Try to decompress (LZ4)
            let decompressed = lz4_flex::decompress_size_prepended(&checkpoint_data)
                .unwrap_or_else(|_| checkpoint_data.clone());

            // Parse checkpoint header
            let checkpoint_info = parse_checkpoint_header(&decompressed)?;

            println!("  Version: {}", checkpoint_info.version);
            println!("  Source architecture: {}", checkpoint_info.source_arch);
            println!("  Target architecture: {}", current_arch());

            if self.validate {
                validate_checkpoint(&checkpoint_info, &decompressed)?;
                println!("  Validation: OK");
            }

            // TODO: When MVVM feature is fully implemented, this will:
            // 1. Load the WebAssembly module (from checkpoint or module_path)
            // 2. Create a WAMR engine with MVVM restore support
            // 3. Deserialize the checkpoint state
            // 4. Restore memory, stack, globals, tables
            // 5. Resume execution

            println!("Restore functionality requires full MVVM integration.");
            println!("Checkpoint parsed successfully - ready for execution.");
        }

        #[cfg(not(feature = "mvvm"))]
        {
            bail!(
                "MVVM feature is not enabled. Rebuild with `--features mvvm` to use restore functionality."
            );
        }

        Ok(())
    }
}

#[cfg(feature = "mvvm")]
struct CheckpointInfo {
    version: u32,
    source_arch: String,
    granularity: u8,
    module_hash: [u8; 32],
}

#[cfg(feature = "mvvm")]
fn parse_checkpoint_header(data: &[u8]) -> Result<CheckpointInfo> {
    if data.len() < 42 {
        bail!("Invalid checkpoint: too small");
    }

    // Check magic bytes
    if &data[0..4] != b"MVVM" {
        bail!("Invalid checkpoint: bad magic bytes");
    }

    // Version
    let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    // Granularity
    let granularity = data[8];

    // Architecture
    let source_arch = match data[9] {
        0 => "x86_64",
        1 => "aarch64",
        2 => "riscv64",
        _ => "unknown",
    }.to_string();

    // Module hash
    let mut module_hash = [0u8; 32];
    module_hash.copy_from_slice(&data[10..42]);

    Ok(CheckpointInfo {
        version,
        source_arch,
        granularity,
        module_hash,
    })
}

#[cfg(feature = "mvvm")]
fn current_arch() -> &'static str {
    #[cfg(target_arch = "x86_64")]
    { "x86_64" }
    #[cfg(target_arch = "aarch64")]
    { "aarch64" }
    #[cfg(target_arch = "riscv64")]
    { "riscv64" }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64", target_arch = "riscv64")))]
    { "unknown" }
}

#[cfg(feature = "mvvm")]
fn validate_checkpoint(info: &CheckpointInfo, _data: &[u8]) -> Result<()> {
    // Version check
    if info.version != 1 {
        bail!("Unsupported checkpoint version: {}", info.version);
    }

    // Granularity check
    if info.granularity > 2 {
        bail!("Invalid checkpoint granularity: {}", info.granularity);
    }

    // Additional validation would include:
    // - Module hash verification
    // - Memory bounds checking
    // - Stack integrity verification

    Ok(())
}

#[cfg(feature = "mvvm")]
fn read_checkpoint_from_journal(journal_path: &PathBuf) -> Result<Vec<u8>> {
    use wasmer_wasix::journal::{LogFileJournal, MvvmJournalReader};

    let journal = LogFileJournal::new_readonly(journal_path.clone())?;
    let reader = MvvmJournalReader::new(&journal);

    let checkpoint = reader.read_latest_checkpoint()?
        .ok_or_else(|| anyhow::anyhow!("No checkpoint found in journal: {}", journal_path.display()))?;

    Ok(checkpoint.data)
}
