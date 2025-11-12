use std::collections::HashSet;
use std::env;
use std::f32::consts::TAU;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use bitflags::bitflags;
use font8x8::legacy::BASIC_LEGACY;
use rvz::Rvz;
use sdl2::audio::{AudioQueue, AudioSpecDesired};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::PixelFormatEnum;
use sdl2::render::{Canvas, Texture};
use sdl2::video::Window;

use which::which;

use crate::controller::{ControllerManager, VirtualButton};

const DEFAULT_WIDTH: u32 = 640;
const DEFAULT_HEIGHT: u32 = 480;
const TARGET_FRAME: Duration = Duration::from_micros(16_667);
const AUDIO_SAMPLE_RATE: i32 = 48_000;
const AUDIO_CHANNELS: u16 = 2;
const AUDIO_BUFFER_SAMPLES: u16 = 1024;
const MAX_AUDIO_LATENCY_BYTES: u32 =
    (AUDIO_SAMPLE_RATE as u32) * (AUDIO_CHANNELS as u32) * std::mem::size_of::<i16>() as u32;

fn try_launch_dolphin(rom_path: &Path) -> Result<bool> {
    let Some(binary) = resolve_dolphin_binary()? else {
        log::debug!("Dolphin binary not found; falling back to built-in stub core");
        return Ok(false);
    };
    log::info!(
        "Launching Dolphin core via {} for {}",
        binary.display(),
        rom_path.display()
    );
    let mut command = Command::new(&binary);
    if dolphin_supports_batch(&binary) {
        command.arg("-b");
    }
    let status = command
        .arg("-e")
        .arg(rom_path)
        .status()
        .with_context(|| format!("failed to spawn Dolphin binary at {}", binary.display()))?;
    if !status.success() {
        return Err(anyhow::anyhow!(
            "Dolphin exited with status {status}; check the external emulator logs"
        ));
    }
    Ok(true)
}

fn resolve_dolphin_binary() -> Result<Option<PathBuf>> {
    if let Ok(path) = env::var("DOLPHIN_BIN") {
        if !path.is_empty() {
            let bin = PathBuf::from(path);
            if bin.exists() {
                return Ok(Some(bin));
            }
        }
    }
    for candidate in dolphin_candidate_bins() {
        if let Ok(path) = which(candidate) {
            return Ok(Some(path));
        }
    }
    for location in dolphin_candidate_paths() {
        if location.exists() {
            return Ok(Some(location));
        }
    }
    Ok(None)
}

fn dolphin_candidate_bins() -> &'static [&'static str] {
    &[
        "dolphin-emu-nogui",
        "dolphin-emu",
        "dolphin",
        "Dolphin",
        "Dolphin.exe",
    ]
}

fn dolphin_supports_batch(binary: &Path) -> bool {
    binary
        .file_name()
        .and_then(|s| s.to_str())
        .map(|name| !name.to_ascii_lowercase().contains("nogui"))
        .unwrap_or(true)
}

fn dolphin_candidate_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    paths.push(PathBuf::from(
        "/Applications/Dolphin.app/Contents/MacOS/Dolphin",
    ));
    paths.push(PathBuf::from("C:\\Program Files\\Dolphin\\Dolphin.exe"));
    paths.push(PathBuf::from(
        "C:\\Program Files (x86)\\Dolphin\\Dolphin.exe",
    ));
    paths
}

fn load_gamecube_image(path: &Path) -> Result<Vec<u8>> {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("rvz") => load_rvz_image(path),
        _ => fs::read(path)
            .with_context(|| format!("failed to read GameCube image {}", path.display())),
    }
}

fn load_rvz_image(path: &Path) -> Result<Vec<u8>> {
    let file =
        File::open(path).with_context(|| format!("failed to open RVZ image {}", path.display()))?;
    let mut rvz = Rvz::new(file).map_err(|err| anyhow::anyhow!(err))?;
    let iso_size = rvz.metadata.header.iso_file_size;
    if iso_size > (usize::MAX as u64) {
        return Err(anyhow::anyhow!("RVZ image too large to load into memory"));
    }
    let mut buffer = Vec::with_capacity(iso_size as usize);
    rvz.read_to_end(&mut buffer)
        .with_context(|| format!("failed to decompress {}", path.display()))?;
    Ok(buffer)
}

pub fn run(rom_path: &Path, scale: u32, limit_fps: bool) -> Result<()> {
    if try_launch_dolphin(rom_path)? {
        return Ok(());
    }
    let title = rom_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("GameCube");
    let mut core = GamecubeCore::from_disc(rom_path)?;
    let meta = core.metadata();
    log::info!(
        "Loaded GameCube image \"{}\" (ID {} / maker {}) disc {} version {} streaming={}",
        meta.title,
        meta.game_code,
        meta.maker_code,
        meta.disc_number.saturating_add(1),
        meta.version,
        if meta.streaming { "on" } else { "off" }
    );
    let window_title = format!("{} ({})", title, meta.game_code);
    let mut frontend = GamecubeFrontend::new(&window_title, scale.max(1), limit_fps)?;
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
    pressed: HashSet<Keycode>,
    controller: ControllerManager,
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
        let controller = ControllerManager::new(&sdl)?;
        Ok(Self {
            _sdl: sdl,
            canvas,
            texture,
            event_pump,
            audio,
            limit_fps,
            scale,
            argb_buffer: vec![0; (DEFAULT_WIDTH as usize) * (DEFAULT_HEIGHT as usize)],
            pressed: HashSet::new(),
            controller,
        })
    }

    fn run(&mut self, core: &mut GamecubeCore) -> Result<()> {
        let mut running = true;
        let mut last_frame = Instant::now();

        while running {
            for event in self.event_pump.poll_iter() {
                self.controller.handle_event(&event);
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

            let input = self.build_input();
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

    fn build_input(&self) -> GamecubeInput {
        let analog = self.controller.analog_state().unwrap_or_default();
        let mut buttons = PadButton::empty();

        let mut left = AnalogStick {
            x: Self::apply_deadzone(analog.left_x),
            y: Self::apply_deadzone(analog.left_y),
        };
        let mut right = AnalogStick {
            x: Self::apply_deadzone(analog.right_x),
            y: Self::apply_deadzone(analog.right_y),
        };
        let mut trigger_l = analog.left_trigger;
        let mut trigger_r = analog.right_trigger;

        let controller_button_map = [
            (VirtualButton::A, PadButton::A),
            (VirtualButton::B, PadButton::B),
            (VirtualButton::X, PadButton::X),
            (VirtualButton::Y, PadButton::Y),
            (VirtualButton::L, PadButton::L),
            (VirtualButton::R, PadButton::R),
            (VirtualButton::Start, PadButton::START),
            (VirtualButton::Select, PadButton::Z),
        ];
        for (virtual_button, pad_button) in controller_button_map {
            if self.controller.is_pressed(virtual_button) {
                buttons |= pad_button;
            }
        }

        let keyboard_button_map = [
            (Keycode::X, PadButton::A),
            (Keycode::Z, PadButton::B),
            (Keycode::S, PadButton::X),
            (Keycode::A, PadButton::Y),
            (Keycode::Q, PadButton::L),
            (Keycode::W, PadButton::R),
            (Keycode::E, PadButton::Z),
        ];
        for (key, pad_button) in keyboard_button_map {
            if self.key_down(key) {
                buttons |= pad_button;
            }
        }
        if self.any_keys(&[Keycode::Return, Keycode::KpEnter]) {
            buttons |= PadButton::START;
        }
        if self.any_keys(&[Keycode::LShift, Keycode::RShift]) {
            buttons |= PadButton::Z;
        }

        let dpad_left =
            self.key_down(Keycode::Left) || self.controller.is_pressed(VirtualButton::Left);
        let dpad_right =
            self.key_down(Keycode::Right) || self.controller.is_pressed(VirtualButton::Right);
        let dpad_up = self.key_down(Keycode::Up) || self.controller.is_pressed(VirtualButton::Up);
        let dpad_down =
            self.key_down(Keycode::Down) || self.controller.is_pressed(VirtualButton::Down);

        if dpad_left {
            buttons |= PadButton::DPAD_LEFT;
        }
        if dpad_right {
            buttons |= PadButton::DPAD_RIGHT;
        }
        if dpad_up {
            buttons |= PadButton::DPAD_UP;
        }
        if dpad_down {
            buttons |= PadButton::DPAD_DOWN;
        }

        left.x = Self::axis_override(left.x, dpad_left, dpad_right);
        left.y = Self::axis_override(left.y, dpad_up, dpad_down);

        let c_left = self.key_down(Keycode::J);
        let c_right = self.key_down(Keycode::L);
        let c_up = self.key_down(Keycode::I);
        let c_down = self.key_down(Keycode::K);
        right.x = Self::axis_override(right.x, c_left, c_right);
        right.y = Self::axis_override(right.y, c_up, c_down);

        if self.key_down(Keycode::U) {
            trigger_l = 1.0;
        }
        if self.key_down(Keycode::O) {
            trigger_r = 1.0;
        }

        if buttons.contains(PadButton::L) {
            trigger_l = trigger_l.max(1.0);
        }
        if buttons.contains(PadButton::R) {
            trigger_r = trigger_r.max(1.0);
        }

        GamecubeInput {
            buttons,
            left_stick: left,
            right_stick: right,
            trigger_l: trigger_l.clamp(0.0, 1.0),
            trigger_r: trigger_r.clamp(0.0, 1.0),
        }
    }

    fn key_down(&self, key: Keycode) -> bool {
        self.pressed.contains(&key)
    }

    fn any_keys(&self, keys: &[Keycode]) -> bool {
        keys.iter().copied().any(|key| self.key_down(key))
    }

    fn axis_override(base: f32, negative: bool, positive: bool) -> f32 {
        match (negative, positive) {
            (true, false) => -1.0,
            (false, true) => 1.0,
            (true, true) => 0.0,
            (false, false) => base,
        }
    }

    fn apply_deadzone(value: f32) -> f32 {
        if value.abs() < 0.12 {
            0.0
        } else {
            value.clamp(-1.0, 1.0)
        }
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

#[derive(Debug, Clone)]
struct GamecubeMetadata {
    game_code: String,
    maker_code: String,
    disc_number: u8,
    version: u8,
    streaming: bool,
    title: String,
    overlay_lines: [String; 3],
}

impl GamecubeMetadata {
    fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 0x60 {
            anyhow::bail!("GameCube image is missing the boot header");
        }
        let game_code = decode_ascii(&bytes[0..4]);
        let maker_code = decode_ascii(&bytes[4..6]);
        let disc_number = bytes[6];
        let version = bytes[7];
        let streaming = bytes[8] != 0;
        let title_end = bytes.len().min(0x20 + 0x80);
        let raw_title = decode_ascii(&bytes[0x20..title_end]);
        let title = if raw_title.is_empty() {
            "Unknown Title".to_string()
        } else {
            raw_title
        };
        let overlay_title = shorten_text(&title, 40);
        let safe_game_code = if game_code.is_empty() {
            "----".to_string()
        } else {
            game_code.clone()
        };
        let safe_maker = if maker_code.is_empty() {
            "--".to_string()
        } else {
            maker_code.clone()
        };
        let overlay_lines = [
            format!("TITLE {}", overlay_title),
            format!("ID {}  MAKER {}", safe_game_code, safe_maker),
            format!(
                "DISC {}  VER {}  STREAM {}",
                disc_number.saturating_add(1),
                version,
                if streaming { "ON" } else { "OFF" }
            ),
        ];
        Ok(Self {
            game_code,
            maker_code,
            disc_number,
            version,
            streaming,
            title,
            overlay_lines,
        })
    }
}

fn decode_ascii(slice: &[u8]) -> String {
    let end = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
    let trimmed = &slice[..end];
    String::from_utf8_lossy(trimmed).trim().to_string()
}

fn shorten_text(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let keep = max_chars.saturating_sub(3);
    let mut shortened = String::new();
    for (i, ch) in input.chars().enumerate() {
        if i >= keep {
            break;
        }
        shortened.push(ch);
    }
    shortened.push_str("...");
    shortened
}

struct GamecubeCore {
    _disc_bytes: Vec<u8>,
    width: u32,
    height: u32,
    frame_buffer: Vec<u32>,
    audio_buffer: Vec<i16>,
    audio_phase: f32,
    frame_counter: u64,
    metadata: GamecubeMetadata,
}

impl GamecubeCore {
    fn from_disc(path: &Path) -> Result<Self> {
        let disc_bytes = load_gamecube_image(path)?;
        let metadata = GamecubeMetadata::parse(&disc_bytes)?;
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
            metadata,
        })
    }

    fn metadata(&self) -> &GamecubeMetadata {
        &self.metadata
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
        self.overlay_metadata();
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

    fn overlay_metadata(&mut self) {
        const PADDING_X: usize = 14;
        const PADDING_Y: usize = 14;
        const LINE_HEIGHT: usize = 9;
        let lines = self.metadata.overlay_lines.clone();
        let max_chars = lines.iter().map(|line| line.len()).max().unwrap_or(0);
        let capped_chars = max_chars.min(60);
        let rect_width = capped_chars.saturating_mul(8) + PADDING_X * 2;
        let rect_height = lines.len().saturating_mul(LINE_HEIGHT) + PADDING_Y;
        self.fill_rect(8, 8, rect_width, rect_height, 0xC010_1010);
        for (idx, line) in lines.iter().enumerate() {
            let y = 16 + idx * LINE_HEIGHT;
            self.draw_text_line(18, y, line, 0xFFFF_FFF0);
        }
    }

    fn fill_rect(
        &mut self,
        start_x: usize,
        start_y: usize,
        width: usize,
        height: usize,
        color: u32,
    ) {
        let frame_width = self.width as usize;
        let frame_height = self.height as usize;
        let max_y = (start_y + height).min(frame_height);
        let max_x = (start_x + width).min(frame_width);
        for y in start_y..max_y {
            let row_offset = y * frame_width;
            for x in start_x..max_x {
                self.frame_buffer[row_offset + x] = color;
            }
        }
    }

    fn draw_text_line(&mut self, start_x: usize, start_y: usize, text: &str, color: u32) {
        for (idx, ch) in text.chars().enumerate() {
            self.draw_char(start_x + idx * 8, start_y, ch, color);
        }
    }

    fn draw_char(&mut self, start_x: usize, start_y: usize, ch: char, color: u32) {
        if ch == ' ' {
            return;
        }
        let glyph = glyph_for(ch);
        let frame_width = self.width as usize;
        let frame_height = self.height as usize;
        for (row, row_bits) in glyph.iter().enumerate() {
            let y = start_y + row;
            if y >= frame_height {
                break;
            }
            for col in 0..8 {
                if (row_bits >> col) & 1 == 0 {
                    continue;
                }
                let x = start_x + col;
                if x >= frame_width {
                    continue;
                }
                self.frame_buffer[y * frame_width + x] = color;
            }
        }
    }
}

fn glyph_for(ch: char) -> [u8; 8] {
    let idx = ch as usize;
    if idx < BASIC_LEGACY.len() {
        BASIC_LEGACY[idx]
    } else {
        BASIC_LEGACY['?' as usize]
    }
}
