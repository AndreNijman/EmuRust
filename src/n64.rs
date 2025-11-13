use std::path::Path;

use anyhow::{Result, bail};

pub fn run(rom_path: &Path, _scale: u32, _limit_fps: bool) -> Result<()> {
    bail!(
        "Nintendo 64 core integration for {} is not implemented yet",
        rom_path.display()
    );
}
