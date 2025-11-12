use std::f32::consts::TAU;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use bitflags::bitflags;
use sdl2::audio::{AudioQueue, AudioSpecDesired};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::PixelFormatEnum;
use sdl2::render::{Canvas, Texture};
use sdl2::video::Window;

const DEFAULT_WIDTH: u32 = 640;
const DEFAULT_HEIGHT: u32 = 480;
const TARGET_FRAME: Duration = Duration::from_micros(16_667);
const AUDIO_SAMPLE_RATE: i32 = 48_000;
const AUDIO_CHANNELS: u16 = 2;
const AUDIO_BUFFER_SAMPLES: u16 = 1024;
const MAX_AUDIO_LATENCY_BYTES: u32 =
    (AUDIO_SAMPLE_RATE as u32) * (AUDIO_CHANNELS as u32) * std::mem::size_of::<i16>() as u32;

pub fn run(rom_path: &Path, scale: u32, limit_fps: bool) -> Result<()> {
    let title = rom_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("GameCube");
    let mut core = GamecubeCore::from_disc(rom_path)?;
    let mut frontend = GamecubeFrontend::new(title, scale.max(1), limit_fps)?;
    frontend.run(&mut core)
}

struct GamecubeFrontend {
    _sdl: sdl2::Sdl,
    canvas: Canvas<Window>,
    texture: Texture,
    event_pump: sdl2::EventPump,
    audio: AudioQueue<i16>,
    limit_fps: bool,
    scale: u32,
    argb_buffer: Vec<u32>,
}

impl GamecubeFrontend {
    fn new(title: &str, scale: u32, limit_fps: bool) -> Result<Self> {
        let sdl = sdl2::init().map_err(|e| anyhow::anyhow!(e))?;
        let video = sdl.video().map_err(|e| anyhow::anyhow!(e))?;
        let audio_subsystem = sdl.audio().map_err(|e| anyhow::anyhow!(e))?;

        let width = DEFAULT_WIDTH.saturating_mul(scale);
        let height = DEFAULT_HEIGHT.saturating_mul(scale);
        let window = video
            .window(title, width, height)
            .position_centered()
            .resizable()
            .build()
            .context("failed to create GameCube window")?;

        let mut canvas_builder = window.into_canvas();
        if limit_fps {
            canvas_builder = canvas_builder.present_vsync();
        }
        let mut canvas = canvas_builder.build().map_err(|e| anyhow::anyhow!(e))?;
        canvas
            .set_logical_size(DEFAULT_WIDTH, DEFAULT_HEIGHT)
            .context("failed to set GameCube logical size")?;

        let texture_creator = canvas.texture_creator();
        let texture = texture_creator
            .create_texture_streaming(PixelFormatEnum::ARGB8888, DEFAULT_WIDTH, DEFAULT_HEIGHT)
            .map_err(|e| anyhow::anyhow!(e))?;

        let desired = AudioSpecDesired {
            freq: Some(AUDIO_SAMPLE_RATE),
            channels: Some(AUDIO_CHANNELS as u8),
            samples: Some(AUDIO_BUFFER_SAMPLES),
        };
        let audio = audio_subsystem
            .open_queue::<i16, _>(None, &desired)
            .map_err(|e| anyhow::anyhow!(e))?;
        audio.resume();

        let event_pump = sdl.event_pump().map_err(|e| anyhow::anyhow!(e))?;
        Ok(Self {
            _sdl: sdl,
            canvas,
            texture,
            event_pump,
            audio,
            limit_fps,
            scale,
            argb_buffer: vec![0; (DEFAULT_WIDTH as usize) * (DEFAULT_HEIGHT as usize)],
        })
    }

    fn run(&mut self, core: &mut GamecubeCore) -> Result<()> {
        let mut running = true;
        let mut last_frame = Instant::now();
        let input = GamecubeInput::default();

        while running {
            for event in self.event_pump.poll_iter() {
                match event {
                    Event::Quit { .. } => running = false,
                    Event::KeyDown {
                        keycode: Some(Keycode::Escape),
                        ..
                    } => running = false,
                    _ => {}
                }
            }

            core.step(&input);
            let (width, height) = core.dimensions();
            self.present_frame(core.frame(), width, height)?;
            self.push_audio(core.audio())?;

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

    fn present_frame(&mut self, frame: &[u32], width: u32, height: u32) -> Result<()> {
        let expected_len = width
            .saturating_mul(height)
            .try_into()
            .unwrap_or(self.argb_buffer.len());
        if frame.len() != expected_len {
            anyhow::bail!(
                "frame buffer size mismatch (expected {}, got {})",
                expected_len,
                frame.len()
            );
        }

        if self.argb_buffer.len() != frame.len() {
            self.argb_buffer.resize(frame.len(), 0);
        }

        self.argb_buffer.copy_from_slice(frame);
        self.texture
            .update(
                None,
                bytemuck::cast_slice(&self.argb_buffer),
                width as usize * 4,
            )
            .context("failed to upload GameCube frame")?;

        if let Err(err) = self.canvas.set_logical_size(width, height) {
            log::debug!("failed to update GameCube logical size: {err}");
        }
        let scaled_w = width.saturating_mul(self.scale);
        let scaled_h = height.saturating_mul(self.scale);
        if let Err(err) = self.canvas.window_mut().set_size(scaled_w, scaled_h) {
            log::debug!("failed to resize GameCube window: {err}");
        }

        self.canvas.clear();
        self.canvas
            .copy(&self.texture, None, None)
            .map_err(|e| anyhow::anyhow!(e))?;
        self.canvas.present();
        Ok(())
    }

    fn push_audio(&mut self, samples: &[i16]) -> Result<()> {
        if samples.is_empty() {
            return Ok(());
        }
        let queued = self.audio.size() as u32;
        if queued > MAX_AUDIO_LATENCY_BYTES {
            self.audio.clear();
        }
        self.audio
            .queue_audio(samples)
            .map_err(|e| anyhow::anyhow!(e))
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default)]
    pub struct PadButton: u16 {
        const A = 0b0000_0000_0001;
        const B = 0b0000_0000_0010;
        const X = 0b0000_0000_0100;
        const Y = 0b0000_0000_1000;
        const START = 0b0000_0001_0000;
        const DPAD_UP = 0b0000_0010_0000;
        const DPAD_DOWN = 0b0000_0100_0000;
        const DPAD_LEFT = 0b0000_1000_0000;
        const DPAD_RIGHT = 0b0001_0000_0000;
        const L = 0b0010_0000_0000;
        const R = 0b0100_0000_0000;
        const Z = 0b1000_0000_0000;
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AnalogStick {
    pub x: f32,
    pub y: f32,
}

impl Default for AnalogStick {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GamecubeInput {
    pub buttons: PadButton,
    pub left_stick: AnalogStick,
    pub right_stick: AnalogStick,
    pub trigger_l: f32,
    pub trigger_r: f32,
}

struct GamecubeCore {
    _disc_bytes: Vec<u8>,
    width: u32,
    height: u32,
    frame_buffer: Vec<u32>,
    audio_buffer: Vec<i16>,
    audio_phase: f32,
    frame_counter: u64,
}

impl GamecubeCore {
    fn from_disc(path: &Path) -> Result<Self> {
        let disc_bytes = fs::read(path)
            .with_context(|| format!("failed to read GameCube image {}", path.display()))?;
        let width = DEFAULT_WIDTH;
        let height = DEFAULT_HEIGHT;
        Ok(Self {
            _disc_bytes: disc_bytes,
            width,
            height,
            frame_buffer: vec![0; (width as usize) * (height as usize)],
            audio_buffer: Vec::with_capacity((AUDIO_BUFFER_SAMPLES as usize) * 2),
            audio_phase: 0.0,
            frame_counter: 0,
        })
    }

    fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn frame(&self) -> &[u32] {
        &self.frame_buffer
    }

    fn audio(&self) -> &[i16] {
        &self.audio_buffer
    }

    fn step(&mut self, input: &GamecubeInput) {
        self.frame_counter = self.frame_counter.wrapping_add(1);
        self.render_pattern(input);
        self.generate_audio(input);
    }

    fn render_pattern(&mut self, input: &GamecubeInput) {
        let width = self.width as usize;
        let height = self.height as usize;
        let frame = &mut self.frame_buffer;
        let t = (self.frame_counter as f32) * 0.02;
        let left_x = input.left_stick.x;
        let left_y = input.left_stick.y;
        let right_x = input.right_stick.x;
        let trigger = (input.trigger_l + input.trigger_r).clamp(0.0, 2.0);

        for y in 0..height {
            let yf = y as f32 / height as f32;
            for x in 0..width {
                let xf = x as f32 / width as f32;
                let swirl = ((xf * 10.0 + t) + (yf * 10.0 - t)).sin();
                let pulse = ((self.frame_counter as f32 * 0.1) + xf * 15.0).cos();
                let stick = (left_x * xf + left_y * yf) * 0.5;
                let shimmer = (right_x * 0.5 + pulse * 0.5).sin();

                let r = (((swirl + 1.0) * 0.5 + stick) * 255.0).clamp(0.0, 255.0) as u32;
                let g = (((pulse + 1.0) * 0.5 + trigger * 0.25) * 255.0).clamp(0.0, 255.0) as u32;
                let b = (((shimmer + 1.0) * 0.5) * 255.0).clamp(0.0, 255.0) as u32;

                frame[y * width + x] = 0xFF00_0000 | (r << 16) | (g << 8) | b;
            }
        }
    }

    fn generate_audio(&mut self, input: &GamecubeInput) {
        let mut frequency = 220.0 + (input.left_stick.x * 80.0);
        if input.buttons.contains(PadButton::A) {
            frequency += 120.0;
        }
        if input.buttons.contains(PadButton::B) {
            frequency -= 60.0;
        }
        if input.buttons.contains(PadButton::START) {
            frequency += 30.0 * (self.frame_counter as f32 % 3.0);
        }
        frequency = frequency.clamp(110.0, 880.0);

        self.audio_buffer.clear();
        let frames = (AUDIO_SAMPLE_RATE as f32 / 60.0).round().max(1.0) as usize;
        for _ in 0..frames {
            let sample = (self.audio_phase).sin() * 0.2;
            let value = (sample * i16::MAX as f32) as i16;
            for _ in 0..AUDIO_CHANNELS {
                self.audio_buffer.push(value);
            }
            let phase_increment = TAU * frequency / AUDIO_SAMPLE_RATE as f32;
            self.audio_phase = (self.audio_phase + phase_increment).rem_euclid(TAU);
        }
    }
}
