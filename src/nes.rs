use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

const NES_ENV: &str = "NES_EMULATOR";

pub fn run(rom: &Path) -> Result<()> {
    let emulator = std::env::var(NES_ENV).context(
        "Set NES_EMULATOR to the path of an NES emulator binary (for example fceux or mesen)",
    )?;
    Command::new(emulator)
        .arg(rom)
        .spawn()
        .context("failed to launch NES emulator process")?
        .wait()
        .context("NES emulator process failed")?;
    Ok(())
}
