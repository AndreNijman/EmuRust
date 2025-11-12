use std::path::Path;

use anyhow::{Result, anyhow};

#[derive(Debug, Clone, Copy)]
pub enum GameSystem {
    GameBoy,
    Nes,
    Snes,
    Nds,
    GameCube,
}

pub fn detect_system(path: &Path) -> Result<GameSystem> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .ok_or_else(|| anyhow!("file has no extension"))?;

    match ext.as_str() {
        "gb" | "gbc" => Ok(GameSystem::GameBoy),
        "nes" => Ok(GameSystem::Nes),
        "sfc" | "smc" | "snes" => Ok(GameSystem::Snes),
        "nds" => Ok(GameSystem::Nds),
        "iso" | "gcm" | "gcz" | "gcn" | "ciso" | "dol" | "rvz" => Ok(GameSystem::GameCube),
        other => Err(anyhow!("unsupported ROM extension: {}", other)),
    }
}
