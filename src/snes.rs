use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use meru_interface::{EmulatorCore, InputData};
use sdl2::audio::{AudioQueue, AudioSpecDesired};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::PixelFormatEnum;
use sdl2::render::{Canvas, Texture};
use sdl2::video::Window;
use super_sabicom::Snes;

const TARGET_FRAME: Duration = Duration::from_micros(16_667);
const DEFAULT_WIDTH: u32 = 512;
const DEFAULT_HEIGHT: u32 = 448;
const AUDIO_SAMPLE_RATE: i32 = 32_000;
const AUDIO_CHANNELS: u16 = 2;
const AUDIO_BUFFER_SAMPLES: u16 = 1024;
const MAX_AUDIO_LATENCY_BYTES: u32 =
    (AUDIO_SAMPLE_RATE as u32) * (AUDIO_CHANNELS as u32) * std::mem::size_of::<i16>() as u32;

pub fn run(rom_path: &Path, scale: u32, limit_fps: bool) -> Result<()> {
    let rom_bytes = fs::read(rom_path)
        .with_context(|| format!("failed to read SNES ROM {}", rom_path.display()))?;
    let save_path = rom_path.with_extension("sav");
    let backup = load_backup(&save_path)?;
    let mut snes = Snes::try_from_file(&rom_bytes, backup.as_deref(), &Default::default())
        .context("failed to initialize SNES core")?;

    let title = rom_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("SNES");
    let mut frontend = SnesFrontend::new(title, scale.max(1), limit_fps)?;
    frontend.run(&mut snes)?;

    if let Some(save) = snes.backup() {
        save_backup(&save_path, &save)?;
    }

    Ok(())
}

fn load_backup(path: &Path) -> Result<Option<Vec<u8>>> {
    if path.exists() {
        Ok(Some(fs::read(path).with_context(|| {
            format!("failed to read save {}", path.display())
        })?))
    } else {
        Ok(None)
    }
}

fn save_backup(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory for {}", parent.display()))?;
    }
    fs::write(path, data).with_context(|| format!("failed to write save {}", path.display()))
}

struct SnesFrontend {
    _sdl: sdl2::Sdl,
    canvas: Canvas<Window>,
    texture: Texture,
    event_pump: sdl2::EventPump,
    audio: AudioQueue<i16>,
    pressed: HashSet<Keycode>,
    limit_fps: bool,
    scale: u32,
    argb_buffer: Vec<u32>,
    audio_scratch: Vec<i16>,
}

impl SnesFrontend {
    fn new(title: &str, scale: u32, limit_fps: bool) -> Result<Self> {
        let sdl = sdl2::init().map_err(|e| anyhow!(e))?;
        let video = sdl.video().map_err(|e| anyhow!(e))?;
        let audio_subsystem = sdl.audio().map_err(|e| anyhow!(e))?;

        let scaled_w = DEFAULT_WIDTH.saturating_mul(scale.max(1));
        let scaled_h = DEFAULT_HEIGHT.saturating_mul(scale.max(1));
        let window = video
            .window(title, scaled_w, scaled_h)
            .position_centered()
            .resizable()
            .build()
            .context("failed to create SNES window")?;

        let mut canvas_builder = window.into_canvas();
        if limit_fps {
            canvas_builder = canvas_builder.present_vsync();
        }
        let mut canvas = canvas_builder.build().map_err(|e| anyhow!(e))?;
        canvas
            .set_logical_size(DEFAULT_WIDTH, DEFAULT_HEIGHT)
            .context("failed to set SNES logical size")?;

        let texture_creator = canvas.texture_creator();
        let texture = texture_creator
            .create_texture_streaming(PixelFormatEnum::ARGB8888, DEFAULT_WIDTH, DEFAULT_HEIGHT)
            .map_err(|e| anyhow!(e))?;

        let desired = AudioSpecDesired {
            freq: Some(AUDIO_SAMPLE_RATE),
            channels: Some(AUDIO_CHANNELS as u8),
            samples: Some(AUDIO_BUFFER_SAMPLES),
        };
        let audio = audio_subsystem
            .open_queue::<i16, _>(None, &desired)
            .map_err(|e| anyhow!(e))?;
        let silence = vec![0i16; (AUDIO_BUFFER_SAMPLES as usize) * (AUDIO_CHANNELS as usize)];
        audio.queue_audio(&silence).map_err(|e| anyhow!(e))?;
        audio.resume();

        let event_pump = sdl.event_pump().map_err(|e| anyhow!(e))?;

        Ok(Self {
            _sdl: sdl,
            canvas,
            texture,
            event_pump,
            audio,
            pressed: HashSet::new(),
            limit_fps,
            scale: scale.max(1),
            argb_buffer: vec![0; (DEFAULT_WIDTH as usize) * (DEFAULT_HEIGHT as usize)],
            audio_scratch: Vec::with_capacity(2048),
        })
    }

    fn run(&mut self, snes: &mut Snes) -> Result<()> {
        let mut last_frame = Instant::now();
        let mut running = true;
        while running {
            for event in self.event_pump.poll_iter() {
                match event {
                    Event::Quit { .. } => running = false,
                    Event::KeyDown {
                        keycode: Some(Keycode::Escape),
                        ..
                    } => running = false,
                    Event::KeyDown {
                        keycode: Some(code),
                        repeat: false,
                        ..
                    } => {
                        self.pressed.insert(code);
                    }
                    Event::KeyUp {
                        keycode: Some(code),
                        ..
                    } => {
                        self.pressed.remove(&code);
                    }
                    _ => {}
                }
            }

            let input = build_input_data(&self.pressed);
            snes.set_input(&input);
            snes.exec_frame(true);

            self.present_frame(snes.frame_buffer())?;
            self.push_audio(snes.audio_buffer());

            if self.limit_fps {
                let elapsed = last_frame.elapsed();
                if elapsed < TARGET_FRAME {
                    std::thread::sleep(TARGET_FRAME - elapsed);
                }
            }
            last_frame = Instant::now();
        }
        Ok(())
    }

    fn present_frame(&mut self, frame: &meru_interface::FrameBuffer) -> Result<()> {
        let expected_len = frame.width.saturating_mul(frame.height) as usize;
        if self.argb_buffer.len() != expected_len {
            self.argb_buffer.resize(expected_len, 0);
            self.canvas
                .set_logical_size(frame.width as u32, frame.height as u32)
                .context("failed to update SNES logical size")?;
            let width = frame.width as u32;
            let height = frame.height as u32;
            let new_w = width.saturating_mul(self.scale);
            let new_h = height.saturating_mul(self.scale);
            if let Err(err) = self.canvas.window_mut().set_size(new_w, new_h) {
                log::debug!("failed to resize SNES window: {err}");
            }
            let texture_creator = self.canvas.texture_creator();
            self.texture = texture_creator
                .create_texture_streaming(PixelFormatEnum::ARGB8888, width, height)
                .map_err(|e| anyhow!(e))?;
        }

        for (dst, color) in self.argb_buffer.iter_mut().zip(frame.buffer.iter()) {
            *dst =
                0xFF00_0000 | ((color.r as u32) << 16) | ((color.g as u32) << 8) | color.b as u32;
        }

        self.texture
            .update(
                None,
                bytemuck::cast_slice(&self.argb_buffer),
                frame.width as usize * 4,
            )
            .context("failed to upload SNES frame")?;
        self.canvas.clear();
        self.canvas
            .copy(&self.texture, None, None)
            .map_err(|e| anyhow!(e))?;
        self.canvas.present();
        Ok(())
    }

    fn push_audio(&mut self, audio_buffer: &meru_interface::AudioBuffer) {
        if audio_buffer.samples.is_empty() {
            return;
        }

        self.audio_scratch.clear();
        self.audio_scratch.reserve(audio_buffer.samples.len() * 2);
        for sample in &audio_buffer.samples {
            self.audio_scratch.push(sample.left);
            self.audio_scratch.push(sample.right);
        }

        if self.audio.size() as u32 > MAX_AUDIO_LATENCY_BYTES {
            self.audio.clear();
        }

        if let Err(err) = self.audio.queue_audio(&self.audio_scratch) {
            eprintln!("SNES audio error: {err}");
        }
    }
}

fn build_input_data(pressed: &HashSet<Keycode>) -> InputData {
    let mut controller = Vec::with_capacity(12);
    controller.push(("B".into(), is_pressed(pressed, &[Keycode::Z])));
    controller.push(("Y".into(), is_pressed(pressed, &[Keycode::A])));
    controller.push((
        "Select".into(),
        is_pressed(
            pressed,
            &[
                Keycode::RShift,
                Keycode::LShift,
                Keycode::Space,
                Keycode::Backspace,
            ],
        ),
    ));
    controller.push(("Start".into(), is_pressed(pressed, &[Keycode::Return])));
    controller.push(("Up".into(), is_pressed(pressed, &[Keycode::Up])));
    controller.push(("Down".into(), is_pressed(pressed, &[Keycode::Down])));
    controller.push(("Left".into(), is_pressed(pressed, &[Keycode::Left])));
    controller.push(("Right".into(), is_pressed(pressed, &[Keycode::Right])));
    controller.push(("A".into(), is_pressed(pressed, &[Keycode::X])));
    controller.push(("X".into(), is_pressed(pressed, &[Keycode::S])));
    controller.push(("L".into(), is_pressed(pressed, &[Keycode::Q])));
    controller.push(("R".into(), is_pressed(pressed, &[Keycode::W])));

    InputData {
        controllers: vec![controller],
    }
}

fn is_pressed(pressed: &HashSet<Keycode>, keys: &[Keycode]) -> bool {
    keys.iter().any(|key| pressed.contains(key))
}
