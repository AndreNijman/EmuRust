use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

#[allow(unused_variables)]
pub fn run(
    rom_path: &Path,
    scale: u32,
    limit_fps: bool,
    bios_override: Option<PathBuf>,
) -> Result<()> {
    bail!("PlayStation core integration is still being wired up");
}
