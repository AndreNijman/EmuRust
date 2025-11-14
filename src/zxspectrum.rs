use std::collections::VecDeque;
use std::fs;
use std::io::{Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use bytemuck;
use log::warn;
use rustzx_core::EmulationMode;
use rustzx_core::error::Error as CoreError;
use rustzx_core::host::{
    DataRecorder, FrameBuffer, FrameBufferSource, Host, HostContext, LoadableAsset, RomFormat,
    RomSet, SeekFrom as SpectrumSeek, SeekableAsset, Snapshot, SnapshotRecorder, Stopwatch,
    StubDebugInterface, StubIoExtender, Tape,
};
use rustzx_core::zx::{
    constants::{CANVAS_HEIGHT, CANVAS_WIDTH, FPS},
    joy::kempston::KempstonKey,
    keys::{CompoundKey, ZXKey},
    machine::ZXMachine,
    video::colors::{ZXBrightness, ZXColor},
};
use rustzx_core::{Emulator, RustzxSettings};
use sdl2::event::{Event, WindowEvent};
use sdl2::keyboard::Keycode;
use sdl2::pixels::PixelFormatEnum;
use sdl2::render::{Canvas, Texture};
use sdl2::video::Window;

use crate::controller::{ControllerManager, VirtualButton};

const ZX_SUBDIR: &str = "zx";
const ROM_48K: &str = "48.rom";
const ROM_128K_PART0: &str = "128.rom.0";
const ROM_128K_PART1: &str = "128.rom.1";

const ZX_PALETTE: [[u8; 4]; 16] = [
    0x000000FF_u32.to_be_bytes(),
    0x0000CDFF_u32.to_be_bytes(),
    0xCD0000FF_u32.to_be_bytes(),
    0xCD00CDFF_u32.to_be_bytes(),
    0x00CD00FF_u32.to_be_bytes(),
    0x00CDCDFF_u32.to_be_bytes(),
    0xCDCD00FF_u32.to_be_bytes(),
    0xCDCDCDFF_u32.to_be_bytes(),
    0x000000FF_u32.to_be_bytes(),
    0x0000FFFF_u32.to_be_bytes(),
    0xFF0000FF_u32.to_be_bytes(),
    0xFF00FFFF_u32.to_be_bytes(),
    0x00FF00FF_u32.to_be_bytes(),
    0x00FFFFFF_u32.to_be_bytes(),
    0xFFFF00FF_u32.to_be_bytes(),
    0xFFFFFFFF_u32.to_be_bytes(),
];

const MAX_FRAME_SLICE: Duration = Duration::from_millis(100);
const FRAME_DURATION: Duration = Duration::from_nanos(1_000_000_000u64 / FPS as u64);

pub fn run(rom: &Path, bios_root: &Path, scale: u32, limit_fps: bool) -> Result<()> {
    let assets = SpectrumAssets::load(rom, bios_root)?;
    let SpectrumAssets {
        machine,
        rom_set,
        media,
    } = assets;
    let window_title = rom
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|name| format!("ZX Spectrum - {}", name))
        .unwrap_or_else(|| "ZX Spectrum".to_string());

    let mut emulator = Emulator::<SpectrumHost>::new(
        RustzxSettings {
            machine,
            emulation_mode: EmulationMode::FrameCount(1),
            tape_fastload_enabled: true,
            kempston_enabled: true,
            mouse_enabled: false,
        },
        SpectrumHostContext,
    )
    .map_err(map_core_error)?;
    emulator.load_rom(rom_set).map_err(map_core_error)?;
    match media {
        SpectrumMedia::Tape(tape) => {
            emulator.load_tape(tape).map_err(map_core_error)?;
            emulator.play_tape();
        }
        SpectrumMedia::Snapshot(snapshot) => {
            emulator.load_snapshot(snapshot).map_err(map_core_error)?;
        }
    }
    let mut frontend = SpectrumFrontend::new(&window_title, scale.max(1), limit_fps)?;
    let quick_files = QuickSaveFiles::new(rom);
    frontend.run(&mut emulator, &quick_files)
}

struct SpectrumAssets {
    machine: ZXMachine,
    rom_set: SpectrumRomSet,
    media: SpectrumMedia,
}

impl SpectrumAssets {
    pub fn load(game: &Path, bios_dir: &Path) -> Result<Self> {
        let zx_dir = bios_dir.join(ZX_SUBDIR);
        fs::create_dir_all(&zx_dir)
            .with_context(|| format!("failed to create {}", zx_dir.display()))?;
        let (machine, rom_set) = load_machine_roms(&zx_dir)?;
        let media = SpectrumMedia::load(game)?;
        Ok(Self {
            machine,
            rom_set,
            media,
        })
    }
}

fn load_machine_roms(zx_dir: &Path) -> Result<(ZXMachine, SpectrumRomSet)> {
    let rom128 = (
        find_case_insensitive(zx_dir, ROM_128K_PART0)?,
        find_case_insensitive(zx_dir, ROM_128K_PART1)?,
    );
    if let (Some(part0), Some(part1)) = rom128 {
        let rom0 =
            fs::read(&part0).with_context(|| format!("failed to read {}", part0.display()))?;
        let rom1 =
            fs::read(&part1).with_context(|| format!("failed to read {}", part1.display()))?;
        return Ok((
            ZXMachine::Sinclair128K,
            SpectrumRomSet::new(vec![
                MemoryAsset::from_bytes(rom0),
                MemoryAsset::from_bytes(rom1),
            ]),
        ));
    }

    if let Some(part0) = find_case_insensitive(zx_dir, ROM_48K)? {
        let rom0 =
            fs::read(&part0).with_context(|| format!("failed to read {}", part0.display()))?;
        return Ok((
            ZXMachine::Sinclair48K,
            SpectrumRomSet::new(vec![MemoryAsset::from_bytes(rom0)]),
        ));
    }

    Err(anyhow!(
        "no ZX Spectrum ROMs found under {} (expected {} or {} / {})",
        zx_dir.display(),
        ROM_48K,
        ROM_128K_PART0,
        ROM_128K_PART1
    ))
}

fn find_case_insensitive(dir: &Path, needle: &str) -> Result<Option<PathBuf>> {
    let needle_lower = needle.to_ascii_lowercase();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(anyhow!(
                "failed to scan {} for {}: {}",
                dir.display(),
                needle,
                err
            ));
        }
    };

    for entry in entries {
        let entry = entry?;
        let name_lower = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if name_lower == needle_lower {
            return Ok(Some(entry.path()));
        }
    }
    Ok(None)
}

enum SpectrumMedia {
    Tape(Tape<MemoryAsset>),
    Snapshot(Snapshot<MemoryAsset>),
}

impl SpectrumMedia {
    fn load(path: &Path) -> Result<Self> {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
            .ok_or_else(|| anyhow!("file has no extension"))?;
        let data = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
        match ext.as_str() {
            "tap" => Ok(SpectrumMedia::Tape(Tape::Tap(MemoryAsset::from_bytes(
                data,
            )))),
            "sna" => Ok(SpectrumMedia::Snapshot(Snapshot::Sna(
                MemoryAsset::from_bytes(data),
            ))),
            _ => bail!("unsupported ZX Spectrum format: {}", ext),
        }
    }
}

struct QuickSaveFiles {
    latest: PathBuf,
    previous: PathBuf,
}

impl QuickSaveFiles {
    fn new(rom: &Path) -> Self {
        let stem = rom
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| "zx-spectrum".to_string());
        let latest = PathBuf::from(format!("{}.zx.last.sna", stem));
        let previous = PathBuf::from(format!("{}.zx.prev.sna", stem));
        Self { latest, previous }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpeedSetting {
    Normal,
    Double,
    Unlimited,
}

struct SpectrumFrontend {
    _sdl: sdl2::Sdl,
    canvas: Canvas<Window>,
    texture: Texture,
    event_pump: sdl2::EventPump,
    controller: ControllerManager,
    limit_fps: bool,
}

impl SpectrumFrontend {
    fn new(title: &str, scale: u32, limit_fps: bool) -> Result<Self> {
        let sdl = sdl2::init().map_err(|err| anyhow!(err))?;
        let video = sdl.video().map_err(|err| anyhow!(err))?;
        let width = (CANVAS_WIDTH as u32).saturating_mul(scale.max(1));
        let height = (CANVAS_HEIGHT as u32).saturating_mul(scale.max(1));
        let window = video
            .window(title, width, height)
            .resizable()
            .position_centered()
            .build()
            .context("failed to create ZX Spectrum window")?;
        let mut canvas_builder = window.into_canvas();
        if limit_fps {
            canvas_builder = canvas_builder.present_vsync();
        }
        let mut canvas = canvas_builder.build().map_err(|err| anyhow!(err))?;
        canvas
            .set_logical_size(CANVAS_WIDTH as u32, CANVAS_HEIGHT as u32)
            .context("failed to set ZX Spectrum logical size")?;
        let texture_creator = canvas.texture_creator();
        let texture = texture_creator
            .create_texture_streaming(
                PixelFormatEnum::ARGB8888,
                CANVAS_WIDTH as u32,
                CANVAS_HEIGHT as u32,
            )
            .map_err(|err| anyhow!(err))?;
        let event_pump = sdl.event_pump().map_err(|err| anyhow!(err))?;
        let controller = ControllerManager::new(&sdl)?;
        Ok(Self {
            _sdl: sdl,
            canvas,
            texture,
            event_pump,
            controller,
            limit_fps,
        })
    }

    fn run(
        &mut self,
        emulator: &mut Emulator<SpectrumHost>,
        quick_files: &QuickSaveFiles,
    ) -> Result<()> {
        let mut running = true;
        let mut speed = SpeedSetting::Normal;
        while running {
            let frame_start = Instant::now();
            for event in self.event_pump.poll_iter() {
                self.controller.handle_event(&event);
                match event {
                    Event::Quit { .. }
                    | Event::Window {
                        win_event: WindowEvent::Close,
                        ..
                    } => {
                        running = false;
                        break;
                    }
                    Event::KeyDown {
                        keycode: Some(code),
                        repeat,
                        ..
                    } => {
                        if repeat {
                            continue;
                        }
                        match handle_key_down(emulator, code, quick_files, &mut speed)? {
                            EventControl::Continue => {}
                            EventControl::Quit => {
                                running = false;
                                break;
                            }
                        }
                    }
                    Event::KeyUp {
                        keycode: Some(code),
                        ..
                    } => {
                        handle_key_up(emulator, code);
                    }
                    _ => {}
                }
            }
            if !running {
                break;
            }

            update_controller_bindings(emulator, &self.controller);
            emulator
                .emulate_frames(MAX_FRAME_SLICE)
                .map_err(map_core_error)?;
            self.render(emulator.screen_buffer())?;

            if self.should_throttle(speed) {
                let elapsed = frame_start.elapsed();
                if elapsed < FRAME_DURATION {
                    thread::sleep(FRAME_DURATION - elapsed);
                }
            }
        }
        Ok(())
    }

    fn render(&mut self, framebuffer: &SpectrumFrameBuffer) -> Result<()> {
        self.texture
            .update(
                None,
                bytemuck::cast_slice(framebuffer.argb()),
                CANVAS_WIDTH * 4,
            )
            .context("failed to upload ZX Spectrum frame")?;
        self.canvas.clear();
        self.canvas
            .copy(&self.texture, None, None)
            .map_err(|err| anyhow!(err))?;
        self.canvas.present();
        Ok(())
    }

    fn should_throttle(&self, speed: SpeedSetting) -> bool {
        self.limit_fps && matches!(speed, SpeedSetting::Normal)
    }
}

struct SpectrumRomSet {
    roms: VecDeque<MemoryAsset>,
}

impl SpectrumRomSet {
    fn new(roms: Vec<MemoryAsset>) -> Self {
        Self { roms: roms.into() }
    }
}

impl RomSet for SpectrumRomSet {
    type Asset = MemoryAsset;

    fn format(&self) -> RomFormat {
        RomFormat::Binary16KPages
    }

    fn next_asset(&mut self) -> Option<Self::Asset> {
        self.roms.pop_front()
    }
}

enum EventControl {
    Continue,
    Quit,
}

fn handle_key_down(
    emulator: &mut Emulator<SpectrumHost>,
    keycode: Keycode,
    quick_files: &QuickSaveFiles,
    speed: &mut SpeedSetting,
) -> Result<EventControl> {
    if let Some(control) = process_command_key(emulator, keycode, quick_files, speed)? {
        return Ok(control);
    }
    handle_zx_key(emulator, keycode, true);
    Ok(EventControl::Continue)
}

fn handle_key_up(emulator: &mut Emulator<SpectrumHost>, keycode: Keycode) {
    handle_zx_key(emulator, keycode, false);
}

fn process_command_key(
    emulator: &mut Emulator<SpectrumHost>,
    keycode: Keycode,
    quick_files: &QuickSaveFiles,
    speed: &mut SpeedSetting,
) -> Result<Option<EventControl>> {
    match keycode {
        Keycode::Insert => {
            restart_tape(emulator)?;
            Ok(Some(EventControl::Continue))
        }
        Keycode::Delete => {
            emulator.stop_tape();
            Ok(Some(EventControl::Continue))
        }
        Keycode::F1 => {
            perform_quick_save(emulator, quick_files)?;
            Ok(Some(EventControl::Continue))
        }
        Keycode::F2 => {
            perform_quick_load(emulator, quick_files)?;
            Ok(Some(EventControl::Continue))
        }
        Keycode::F3 => {
            *speed = SpeedSetting::Normal;
            emulator.set_speed(EmulationMode::FrameCount(1));
            Ok(Some(EventControl::Continue))
        }
        Keycode::F4 => {
            *speed = SpeedSetting::Double;
            emulator.set_speed(EmulationMode::FrameCount(2));
            Ok(Some(EventControl::Continue))
        }
        Keycode::F5 => {
            *speed = SpeedSetting::Unlimited;
            emulator.set_speed(EmulationMode::Max);
            Ok(Some(EventControl::Continue))
        }
        Keycode::Escape => Ok(Some(EventControl::Quit)),
        _ => Ok(None),
    }
}

fn handle_zx_key(emulator: &mut Emulator<SpectrumHost>, keycode: Keycode, pressed: bool) {
    if let Some(zx_key) = map_keyboard_key(keycode) {
        emulator.send_key(zx_key, pressed);
    } else if let Some(compound) = map_compound_key(keycode) {
        emulator.send_compound_key(compound, pressed);
    }
}

fn map_keyboard_key(keycode: Keycode) -> Option<ZXKey> {
    match keycode {
        Keycode::LShift | Keycode::RShift => Some(ZXKey::Shift),
        Keycode::Z => Some(ZXKey::Z),
        Keycode::X => Some(ZXKey::X),
        Keycode::C => Some(ZXKey::C),
        Keycode::V => Some(ZXKey::V),
        Keycode::A => Some(ZXKey::A),
        Keycode::S => Some(ZXKey::S),
        Keycode::D => Some(ZXKey::D),
        Keycode::F => Some(ZXKey::F),
        Keycode::G => Some(ZXKey::G),
        Keycode::Q => Some(ZXKey::Q),
        Keycode::W => Some(ZXKey::W),
        Keycode::E => Some(ZXKey::E),
        Keycode::R => Some(ZXKey::R),
        Keycode::T => Some(ZXKey::T),
        Keycode::Num1 => Some(ZXKey::N1),
        Keycode::Num2 => Some(ZXKey::N2),
        Keycode::Num3 => Some(ZXKey::N3),
        Keycode::Num4 => Some(ZXKey::N4),
        Keycode::Num5 => Some(ZXKey::N5),
        Keycode::Num0 => Some(ZXKey::N0),
        Keycode::Num9 => Some(ZXKey::N9),
        Keycode::Num8 => Some(ZXKey::N8),
        Keycode::Num7 => Some(ZXKey::N7),
        Keycode::Num6 => Some(ZXKey::N6),
        Keycode::P => Some(ZXKey::P),
        Keycode::O => Some(ZXKey::O),
        Keycode::I => Some(ZXKey::I),
        Keycode::U => Some(ZXKey::U),
        Keycode::Y => Some(ZXKey::Y),
        Keycode::Return | Keycode::KpEnter => Some(ZXKey::Enter),
        Keycode::L => Some(ZXKey::L),
        Keycode::K => Some(ZXKey::K),
        Keycode::J => Some(ZXKey::J),
        Keycode::H => Some(ZXKey::H),
        Keycode::Space => Some(ZXKey::Space),
        Keycode::LCtrl | Keycode::RCtrl => Some(ZXKey::SymShift),
        Keycode::M => Some(ZXKey::M),
        Keycode::N => Some(ZXKey::N),
        Keycode::B => Some(ZXKey::B),
        _ => None,
    }
}

fn map_compound_key(keycode: Keycode) -> Option<CompoundKey> {
    match keycode {
        Keycode::Left => Some(CompoundKey::ArrowLeft),
        Keycode::Right => Some(CompoundKey::ArrowRight),
        Keycode::Up => Some(CompoundKey::ArrowUp),
        Keycode::Down => Some(CompoundKey::ArrowDown),
        Keycode::CapsLock => Some(CompoundKey::CapsLock),
        Keycode::Backspace => Some(CompoundKey::Delete),
        Keycode::End => Some(CompoundKey::Break),
        _ => None,
    }
}

fn update_controller_bindings(
    emulator: &mut Emulator<SpectrumHost>,
    controllers: &ControllerManager,
) {
    let up = controllers.is_pressed(VirtualButton::Up);
    let down = controllers.is_pressed(VirtualButton::Down);
    let left = controllers.is_pressed(VirtualButton::Left);
    let right = controllers.is_pressed(VirtualButton::Right);
    emulator.send_kempston_key(KempstonKey::Up, up);
    emulator.send_kempston_key(KempstonKey::Down, down);
    emulator.send_kempston_key(KempstonKey::Left, left);
    emulator.send_kempston_key(KempstonKey::Right, right);
    let fire = controllers.is_pressed(VirtualButton::A)
        || controllers.is_pressed(VirtualButton::B)
        || controllers.is_pressed(VirtualButton::X)
        || controllers.is_pressed(VirtualButton::Y);
    emulator.send_kempston_key(KempstonKey::Fire, fire);

    let enter = controllers.is_pressed(VirtualButton::Start);
    let space = controllers.is_pressed(VirtualButton::Select);
    emulator.send_key(ZXKey::Enter, enter);
    emulator.send_key(ZXKey::Space, space);
}

fn restart_tape(emulator: &mut Emulator<SpectrumHost>) -> Result<()> {
    emulator.stop_tape();
    match emulator.rewind_tape().map_err(map_core_error) {
        Ok(()) => emulator.play_tape(),
        Err(err) => {
            warn!("failed to rewind ZX Spectrum tape: {err}");
            return Ok(());
        }
    }
    Ok(())
}

fn perform_quick_save(emulator: &mut Emulator<SpectrumHost>, files: &QuickSaveFiles) -> Result<()> {
    if files.previous.exists() {
        fs::remove_file(&files.previous)
            .with_context(|| format!("failed to remove {}", files.previous.display()))?;
    }
    if files.latest.exists() {
        fs::rename(&files.latest, &files.previous)
            .with_context(|| format!("failed to rotate {}", files.latest.display()))?;
    }
    let file = fs::File::create(&files.latest)
        .with_context(|| format!("failed to create {}", files.latest.display()))?;
    let recorder = SnapshotRecorder::Sna(FileRecorder::new(file));
    emulator.save_snapshot(recorder).map_err(map_core_error)?;
    Ok(())
}

fn perform_quick_load(emulator: &mut Emulator<SpectrumHost>, files: &QuickSaveFiles) -> Result<()> {
    if !files.latest.exists() {
        warn!("quick snapshot {} not found", files.latest.display());
        return Ok(());
    }
    let data = fs::read(&files.latest)
        .with_context(|| format!("failed to read {}", files.latest.display()))?;
    let snapshot = Snapshot::Sna(MemoryAsset::from_bytes(data));
    emulator.load_snapshot(snapshot).map_err(map_core_error)?;
    Ok(())
}

#[derive(Clone)]
struct SpectrumHostContext;

struct SpectrumHost;

impl Host for SpectrumHost {
    type Context = SpectrumHostContext;
    type TapeAsset = MemoryAsset;
    type FrameBuffer = SpectrumFrameBuffer;
    type EmulationStopwatch = InstantStopwatch;
    type IoExtender = StubIoExtender;
    type DebugInterface = StubDebugInterface;
}

impl HostContext<SpectrumHost> for SpectrumHostContext {
    fn frame_buffer_context(&self) -> <SpectrumFrameBuffer as FrameBuffer>::Context {
        SpectrumFrameBufferContext
    }
}

struct InstantStopwatch {
    start: Instant,
}

impl Stopwatch for InstantStopwatch {
    fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    fn measure(&self) -> Duration {
        self.start.elapsed()
    }
}

#[derive(Clone)]
struct SpectrumFrameBufferContext;

struct SpectrumFrameBuffer {
    width: usize,
    pixels: Vec<u32>,
}

impl SpectrumFrameBuffer {
    fn argb(&self) -> &[u32] {
        &self.pixels
    }
}

impl FrameBuffer for SpectrumFrameBuffer {
    type Context = SpectrumFrameBufferContext;

    fn new(
        width: usize,
        height: usize,
        _source: FrameBufferSource,
        _context: Self::Context,
    ) -> Self {
        Self {
            width,
            pixels: vec![0xFF00_0000; width * height],
        }
    }

    fn set_color(&mut self, x: usize, y: usize, color: ZXColor, brightness: ZXBrightness) {
        let idx = y * self.width + x;
        if idx >= self.pixels.len() {
            return;
        }
        let palette_index = (color as usize) + (brightness as usize) * 8;
        let rgba = ZX_PALETTE[palette_index];
        let argb = ((rgba[3] as u32) << 24)
            | ((rgba[0] as u32) << 16)
            | ((rgba[1] as u32) << 8)
            | (rgba[2] as u32);
        self.pixels[idx] = argb;
    }
}

struct FileRecorder {
    file: fs::File,
}

impl FileRecorder {
    fn new(file: fs::File) -> Self {
        Self { file }
    }
}

impl DataRecorder for FileRecorder {
    fn write(&mut self, buf: &[u8]) -> Result<usize, rustzx_core::error::IoError> {
        self.file.write(buf).map_err(map_std_io_error)
    }
}

struct MemoryAsset {
    cursor: Cursor<Vec<u8>>,
}

impl MemoryAsset {
    fn from_bytes(data: Vec<u8>) -> Self {
        Self {
            cursor: Cursor::new(data),
        }
    }
}
impl LoadableAsset for MemoryAsset {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, rustzx_core::error::IoError> {
        self.cursor.read(buf).map_err(map_std_io_error)
    }
}

impl SeekableAsset for MemoryAsset {
    fn seek(&mut self, pos: SpectrumSeek) -> Result<usize, rustzx_core::error::IoError> {
        use std::io::SeekFrom;
        let target = match pos {
            SpectrumSeek::Start(offset) => SeekFrom::Start(offset as u64),
            SpectrumSeek::End(offset) => SeekFrom::End(offset as i64),
            SpectrumSeek::Current(offset) => SeekFrom::Current(offset as i64),
        };
        self.cursor
            .seek(target)
            .map(|val| val as usize)
            .map_err(map_std_io_error)
    }
}

fn map_std_io_error(err: std::io::Error) -> rustzx_core::error::IoError {
    use std::io::ErrorKind;
    match err.kind() {
        ErrorKind::UnexpectedEof => rustzx_core::error::IoError::UnexpectedEof,
        ErrorKind::InvalidInput => rustzx_core::error::IoError::SeekBeforeStart,
        ErrorKind::WriteZero => rustzx_core::error::IoError::WriteZero,
        _ => rustzx_core::error::IoError::HostAssetImplFailed,
    }
}

fn map_core_error(err: CoreError) -> anyhow::Error {
    anyhow!(err.to_string())
}
