mod audio;
mod automation;
mod display;
mod interactive;
mod rtc;

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use gameboy_core::Gameboy;
use gameboy_core::emulator::step_result::StepResult;

use crate::audio::AudioPlayer;
use crate::automation::{
    AutomationRecorder, DumpFormat, MemoryRange, WatchFormat, WatchOutput, WatchSpec,
    parse_range_arg, parse_watch_spec, write_dump,
};
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

    /// Dump a memory range after the run completes (format START:LEN, hex or decimal)
    #[arg(long, value_name = "START:LEN", value_parser = parse_range_arg)]
    dump_range: Option<MemoryRange>,

    /// Output file for the memory dump (stdout when omitted)
    #[arg(long, requires = "dump_range")]
    dump_output: Option<PathBuf>,

    /// Memory dump format
    #[arg(long, value_enum, default_value_t = DumpFormat::Hex, requires = "dump_range")]
    dump_format: DumpFormat,

    /// Memory watch (NAME:START:LEN). Repeat for multiple watches.
    #[arg(long = "watch", value_parser = parse_watch_spec, value_name = "NAME:START:LEN")]
    watches: Vec<WatchSpec>,

    /// Output format for memory watches
    #[arg(long, value_enum, default_value_t = WatchFormat::Json, requires = "watches")]
    watch_format: WatchFormat,

    /// Optional file to collect memory watch output (stdout when omitted)
    #[arg(long, requires = "watches")]
    watch_output: Option<PathBuf>,
}

fn main() -> Result<()> {
    env_logger::init();
    let cli = Cli::parse();
    let rom_bytes = fs::read(&cli.rom).with_context(|| "failed to read ROM file")?;

    let rtc = Box::new(SystemRtc);
    let mut gameboy = Gameboy::from_rom(rom_bytes, rtc).map_err(|err| anyhow!(err))?;

    let mut audio = AudioPlayer::new()?;
    let mut recorder = build_recorder(&cli)?;

    if cli.interactive {
        let title = cli
            .rom
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Game Boy");
        let mut runner = InteractiveRunner::new(title, cli.scale, cli.limit_fps)?;
        runner.run(&mut gameboy, &mut audio, recorder.as_mut())?;
    } else {
        run_headless(&mut gameboy, &cli, &mut audio, recorder.as_mut())?;
    }

    if let Some(range) = &cli.dump_range {
        let data = range.capture(&gameboy);
        write_dump(range, &data, cli.dump_format, cli.dump_output.as_deref())?;
    }

    Ok(())
}

fn build_recorder(cli: &Cli) -> Result<Option<AutomationRecorder>> {
    if cli.watches.is_empty() {
        return Ok(None);
    }
    let output = match &cli.watch_output {
        Some(path) => WatchOutput::File(path.clone()),
        None => WatchOutput::Stdout,
    };
    let recorder = AutomationRecorder::new(cli.watches.clone(), cli.watch_format, output)?;
    Ok(Some(recorder))
}

fn run_headless(
    gameboy: &mut Gameboy,
    cli: &Cli,
    audio: &mut AudioPlayer,
    mut recorder: Option<&mut AutomationRecorder>,
) -> Result<()> {
    let mut display = NullDisplay;
    let mut frame_counter = 0u64;

    if let Some(cycles) = cli.cycles {
        for _ in 0..cycles {
            match gameboy.emulate(&mut display) {
                StepResult::AudioBufferFull => audio.push_samples(gameboy.get_audio_buffer()),
                StepResult::VBlank => {
                    if let Some(rec) = recorder.as_deref_mut() {
                        rec.record(frame_counter, gameboy)?;
                    }
                    frame_counter += 1;
                }
                StepResult::Nothing => {}
            }
        }
        return Ok(());
    }

    if cli.frames == 0 {
        loop {
            match gameboy.emulate(&mut display) {
                StepResult::AudioBufferFull => audio.push_samples(gameboy.get_audio_buffer()),
                StepResult::VBlank => {
                    if let Some(rec) = recorder.as_deref_mut() {
                        rec.record(frame_counter, gameboy)?;
                    }
                    frame_counter += 1;
                }
                StepResult::Nothing => {}
            }
        }
    }

    let mut remaining = cli.frames;
    while remaining > 0 {
        match gameboy.emulate(&mut display) {
            StepResult::VBlank => {
                remaining -= 1;
                if let Some(rec) = recorder.as_deref_mut() {
                    rec.record(frame_counter, gameboy)?;
                }
                frame_counter += 1;
            }
            StepResult::AudioBufferFull => audio.push_samples(gameboy.get_audio_buffer()),
            StepResult::Nothing => {}
        };
    }
    Ok(())
}
