mod audio;
mod display;
mod interactive;
mod launcher;
mod nes;
mod rtc;
mod snes;
mod systems;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use gameboy_core::Gameboy;

use crate::audio::AudioPlayer;
use crate::interactive::InteractiveRunner;
use crate::rtc::SystemRtc;
use crate::systems::{GameSystem, detect_system};

#[derive(Parser, Debug)]
#[command(
    name = "retro-launcher",
    version,
    about = "Multi-system retro game launcher"
)]
struct Cli {
    /// Optional ROM path to skip the launcher menu
    #[arg(long)]
    rom: Option<PathBuf>,

    /// Window scale factor for handheld systems
    #[arg(long, default_value_t = 4)]
    scale: u32,

    /// Limit interactive window to ~60 FPS
    #[arg(long)]
    limit_fps: bool,
}

fn main() -> Result<()> {
    env_logger::init();
    let cli = Cli::parse();
    let games_dir = Path::new("games");
    fs::create_dir_all(games_dir).context("failed to create games directory")?;

    let rom_path = match &cli.rom {
        Some(path) => path.clone(),
        None => launcher::select_game(games_dir)?,
    };

    match detect_system(&rom_path)? {
        GameSystem::GameBoy => run_gameboy(&rom_path, &cli),
        GameSystem::Nes => nes::run(&rom_path),
        GameSystem::Snes => snes::run(&rom_path),
    }
}

fn run_gameboy(rom_path: &Path, cli: &Cli) -> Result<()> {
    let rom_bytes =
        fs::read(rom_path).with_context(|| format!("failed to read {}", rom_path.display()))?;
    let title = rom_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Game Boy");

    let rtc = Box::new(SystemRtc);
    let mut gameboy = Gameboy::from_rom(rom_bytes, rtc).map_err(|err| anyhow!(err))?;
    let mut audio = AudioPlayer::new()?;
    let mut runner = InteractiveRunner::new(title, cli.scale, cli.limit_fps)?;
    runner.run(&mut gameboy, &mut audio)
}
