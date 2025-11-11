mod display;
mod interactive;
mod rtc;

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use gameboy_core::Gameboy;
use gameboy_core::emulator::step_result::StepResult;

use crate::display::NullDisplay;
use crate::interactive::InteractiveRunner;
use crate::rtc::SystemRtc;

#[derive(Parser, Debug)]
#[command(
    name = "gameboy",
    version,
    about = "High-performance Game Boy emulator (Rust)"
)]
struct Cli {
    /// Path to a .gb or .gbc ROM
    rom: PathBuf,

    /// Number of frames to execute (0 = run until interrupted)
    #[arg(long, default_value_t = 60)]
    frames: u64,

    /// Execute an exact number of CPU cycles instead of frames
    #[arg(long)]
    cycles: Option<u64>,

    /// Launch an interactive window with keyboard controls
    #[arg(long)]
    interactive: bool,

    /// Scale factor for the interactive window (0 = fit screen)
    #[arg(long, default_value_t = 4)]
    scale: u32,

    /// Limit the interactive window to ~60 FPS
    #[arg(long)]
    limit_fps: bool,
}

fn main() -> Result<()> {
    env_logger::init();
    let cli = Cli::parse();
    let rom_bytes = fs::read(&cli.rom).with_context(|| "failed to read ROM file")?;

    let rtc = Box::new(SystemRtc);
    let mut gameboy = Gameboy::from_rom(rom_bytes, rtc).map_err(|err| anyhow!(err))?;

    if cli.interactive {
        let title = cli
            .rom
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Game Boy");
        let mut runner = InteractiveRunner::new(title, cli.scale, cli.limit_fps)?;
        runner.run(&mut gameboy)?;
    } else {
        run_headless(&mut gameboy, &cli)?;
    }

    Ok(())
}

fn run_headless(gameboy: &mut Gameboy, cli: &Cli) -> Result<()> {
    let mut display = NullDisplay;
    if let Some(cycles) = cli.cycles {
        for _ in 0..cycles {
            match gameboy.emulate(&mut display) {
                StepResult::AudioBufferFull | StepResult::Nothing => {}
                StepResult::VBlank => {}
            }
        }
        return Ok(());
    }

    if cli.frames == 0 {
        loop {
            match gameboy.emulate(&mut display) {
                StepResult::VBlank | StepResult::AudioBufferFull | StepResult::Nothing => {}
            }
        }
    }

    let mut remaining = cli.frames;
    while remaining > 0 {
        if let StepResult::VBlank = gameboy.emulate(&mut display) {
            remaining -= 1;
        }
    }
    Ok(())
}
