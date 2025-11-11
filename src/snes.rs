use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

const SNES_ENV: &str = "SNES_EMULATOR";

pub fn run(rom: &Path) -> Result<()> {
    let emulator = std::env::var(SNES_ENV).context(
        "Set SNES_EMULATOR to the path of an SNES emulator binary (for example bsnes or snes9x)",
    )?;
    Command::new(emulator)
        .arg(rom)
        .spawn()
        .context("failed to launch SNES emulator process")?
        .wait()
        .context("SNES emulator process failed")?;
    Ok(())
}
