mod audio;
mod controller;
mod display;
mod gamecube;
mod interactive;
mod launcher;
mod n64;
mod nds;
mod nes;
mod ps1;
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

    /// Launch a graphical launcher instead of the terminal menu
    #[arg(long)]
    gui: bool,

    /// Window scale factor for handheld systems
    #[arg(long, default_value_t = 4)]
    scale: u32,

    /// Limit interactive window to ~60 FPS (pass --limit-fps=false to disable)
    #[arg(long, default_value_t = true)]
    limit_fps: bool,

    /// Path to a PlayStation BIOS image (fallbacks to PS1_BIOS/PSX_BIOS env vars + bios/)
    #[arg(long)]
    ps1_bios: Option<PathBuf>,
}

fn main() -> Result<()> {
    env_logger::init();
    let cli = Cli::parse();
    let games_dir = Path::new("games");
    let bios_dir = Path::new("bios");
    fs::create_dir_all(games_dir).context("failed to create games directory")?;
    fs::create_dir_all(bios_dir).context("failed to create bios directory")?;

    let rom_path = match &cli.rom {
        Some(path) => path.clone(),
        None => launcher::select_game(games_dir, cli.gui)?,
    };

    match detect_system(&rom_path)? {
        GameSystem::GameBoy => run_gameboy(&rom_path, &cli),
        GameSystem::Nes => nes::run(&rom_path, cli.scale, cli.limit_fps),
        GameSystem::Snes => snes::run(&rom_path, cli.scale, cli.limit_fps),
        GameSystem::Nds => nds::run(&rom_path, cli.scale, cli.limit_fps),
        GameSystem::Ps1 => ps1::run(&rom_path, cli.scale, cli.limit_fps, cli.ps1_bios.clone()),
        GameSystem::N64 => n64::run(&rom_path, cli.scale, cli.limit_fps),
        GameSystem::GameCube => gamecube::run(&rom_path, cli.scale, cli.limit_fps),
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
