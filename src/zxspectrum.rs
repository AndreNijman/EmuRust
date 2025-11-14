use std::collections::VecDeque;
use std::fs;
use std::io::{Cursor, Read, Seek};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use rustzx_core::error::Error as CoreError;
use rustzx_core::host::{
    FrameBuffer, FrameBufferSource, Host, HostContext, LoadableAsset, RomFormat, RomSet,
    SeekFrom as SpectrumSeek, SeekableAsset, Snapshot, Stopwatch, StubDebugInterface,
    StubIoExtender, Tape,
};
use rustzx_core::EmulationMode;
use rustzx_core::zx::{
    machine::ZXMachine,
    video::colors::{ZXBrightness, ZXColor},
};
use rustzx_core::{Emulator, RustzxSettings};

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

pub fn run(rom: &Path, bios_root: &Path, _scale: u32, _limit_fps: bool) -> Result<()> {
    let assets = SpectrumAssets::load(rom, bios_root)?;
    let SpectrumAssets {
        machine,
        rom_set,
        media,
    } = assets;

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

    bail!("ZX Spectrum front-end is still being wired up")
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
        self.cursor.read(buf).map_err(|err| match err.kind() {
            std::io::ErrorKind::UnexpectedEof => rustzx_core::error::IoError::UnexpectedEof,
            _ => rustzx_core::error::IoError::HostAssetImplFailed,
        })
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
            .map_err(|err| match err.kind() {
                std::io::ErrorKind::UnexpectedEof => rustzx_core::error::IoError::UnexpectedEof,
                std::io::ErrorKind::InvalidInput => rustzx_core::error::IoError::SeekBeforeStart,
                _ => rustzx_core::error::IoError::HostAssetImplFailed,
            })
    }
}

fn map_core_error(err: CoreError) -> anyhow::Error {
    anyhow!(err.to_string())
}
