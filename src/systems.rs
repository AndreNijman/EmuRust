use std::{fmt, path::Path};

use anyhow::{Result, anyhow};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GameSystem {
    GameBoy,
    Nes,
    Snes,
    Nds,
    Ps1,
    N64,
    GameCube,
}

impl GameSystem {
    pub fn label(&self) -> &'static str {
        match self {
            GameSystem::GameBoy => "Game Boy / Color",
            GameSystem::Nes => "NES",
            GameSystem::Snes => "SNES",
            GameSystem::Nds => "Nintendo DS",
            GameSystem::Ps1 => "PlayStation",
            GameSystem::N64 => "Nintendo 64",
            GameSystem::GameCube => "GameCube",
        }
    }
}

impl fmt::Display for GameSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
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
        "cue" | "exe" => Ok(GameSystem::Ps1),
        "n64" | "z64" | "v64" => Ok(GameSystem::N64),
        "iso" | "gcm" | "gcz" | "gcn" | "ciso" | "dol" | "rvz" => Ok(GameSystem::GameCube),
        other => Err(anyhow!("unsupported ROM extension: {}", other)),
    }
}
