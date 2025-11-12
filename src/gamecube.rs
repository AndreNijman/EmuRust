use std::path::Path;

use anyhow::{Result, anyhow};

pub fn run(rom_path: &Path, _scale: u32, _limit_fps: bool) -> Result<()> {
    log::info!("selected GameCube image {}", rom_path.display());
    Err(anyhow!(
        "GameCube support is not yet implemented; stay tuned for the next update"
    ))
}
