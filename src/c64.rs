use std::cell::RefCell;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use bytemuck;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::PixelFormatEnum;
use sdl2::render::{Canvas, Texture};
use sdl2::video::Window;

use zinc64_core::{SoundOutput, VideoOutput};
use zinc64_emu::device::joystick::{Button as JoyButton, Mode as JoystickMode};
use zinc64_emu::device::keyboard::{Key, KeyEvent};
use zinc64_emu::system::{C64, C64Factory, Config};
use zinc64_loader::{Loaders, Reader};

use crate::audio::AudioPlayer;
use crate::controller::{ControllerManager, VirtualButton};

const C64_BIOS_SUBDIR: &str = "c64";
const BASIC_ROM: &str = "basic.rom";
const CHARSET_ROM: &str = "characters.rom";
const KERNAL_ROM: &str = "kernal.rom";

const FRAME_DURATION: Duration = Duration::from_micros((1_000_000f32 / 50.0) as u64);
const PALETTE: [u32; 16] = [
    0x000000FF, 0xFFFFFFFF, 0x68372BFF, 0x70A4B2FF, 0x6F3D86FF, 0x588D43FF, 0x352879FF, 0xB8C76FFF,
    0x6F4F25FF, 0x433900FF, 0x9A6759FF, 0x444444FF, 0x6C6C6CFF, 0x9AD284FF, 0x6C5EB5FF, 0x959595FF,
];

pub fn run(rom: &Path, bios_dir: &Path, scale: u32, limit_fps: bool) -> Result<()> {
    let roms = load_roms(bios_dir)?;
    let mut config = Config::new_with_roms(
        zinc64_core::SystemModel::c64_pal(),
        &roms.basic,
        &roms.charset,
        &roms.kernal,
    );
    config.joystick.joystick_1 = JoystickMode::None;
    config.joystick.joystick_2 = JoystickMode::Joy1;

    let video = Rc::new(RefCell::new(C64FrameBuffer::new(
        config.model.frame_buffer_size.0 as usize,
        config.model.frame_buffer_size.1 as usize,
    )));
    let video_output = video.clone() as Rc<RefCell<dyn VideoOutput>>;

    let audio_sink = Arc::new(C64AudioSink::new()?);
    let sound_output = audio_sink.clone() as Arc<dyn SoundOutput>;

    let config_rc = Rc::new(config);
    let factory = C64Factory::new(config_rc.clone());
    let mut c64 = C64::build(config_rc.clone(), &factory, video_output, sound_output);
    c64.reset(true);
    load_image(&mut c64, rom)?;

    let mut frontend = C64Frontend::new(video, scale.max(1), limit_fps)?;
    frontend.run(&mut c64)
}

struct RomSet {
    basic: Vec<u8>,
    charset: Vec<u8>,
    kernal: Vec<u8>,
}

fn load_roms(bios_dir: &Path) -> Result<RomSet> {
    let rom_dir = bios_dir.join(C64_BIOS_SUBDIR);
    let basic = read_required_rom(&rom_dir, BASIC_ROM)?;
    let charset = read_required_rom(&rom_dir, CHARSET_ROM)?;
    let kernal = read_required_rom(&rom_dir, KERNAL_ROM)?;
    Ok(RomSet {
        basic,
        charset,
        kernal,
    })
}

fn read_required_rom(dir: &Path, name: &str) -> Result<Vec<u8>> {
    let path = dir.join(name);
    fs::read(&path).with_context(|| format!("missing required ROM: {}", path.display()))
}

fn load_image(c64: &mut C64, rom: &Path) -> Result<()> {
    let ext = rom.extension().and_then(|s| s.to_str());
    let loader = Loaders::from_ext(ext).map_err(|err| anyhow!(err))?;
    let file = File::open(rom).with_context(|| format!("failed to open {}", rom.display()))?;
    let mut reader = LoaderFile::new(file);
    let mut autostart = loader.autostart(&mut reader).map_err(|err| anyhow!(err))?;
    autostart.execute(c64);
    Ok(())
}

struct LoaderFile(BufReader<File>);

impl LoaderFile {
    fn new(file: File) -> Self {
        Self(BufReader::new(file))
    }
}

impl Reader for LoaderFile {
    fn read(&mut self, buf: &mut [u8]) -> zinc64_loader::Result<usize> {
        self.0.read(buf).map_err(|err| format!("{}", err))
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> zinc64_loader::Result<usize> {
        self.0.read_to_end(buf).map_err(|err| format!("{}", err))
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> zinc64_loader::Result<()> {
        self.0.read_exact(buf).map_err(|err| format!("{}", err))
    }

    fn consume(&mut self, amt: usize) {
        self.0.consume(amt)
    }
}

struct C64FrameBuffer {
    width: usize,
    height: usize,
    pixels: Vec<u32>,
}

impl C64FrameBuffer {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width * height],
        }
    }

    fn pixels(&self) -> &[u32] {
        &self.pixels
    }
}

impl VideoOutput for C64FrameBuffer {
    fn get_dimension(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn reset(&mut self) {
        for pixel in self.pixels.iter_mut() {
            *pixel = 0;
        }
    }

    fn write(&mut self, index: usize, color: u8) {
        let mapped = PALETTE[color as usize % PALETTE.len()];
        if index < self.pixels.len() {
            self.pixels[index] = mapped;
        }
    }
}

struct C64AudioSink {
    player: Mutex<AudioPlayer>,
}

impl C64AudioSink {
    fn new() -> Result<Self> {
        Ok(Self {
            player: Mutex::new(AudioPlayer::new()?),
        })
    }
}

impl SoundOutput for C64AudioSink {
    fn reset(&self) {
        if let Ok(mut player) = self.player.lock() {
            player.clear();
        }
    }

    fn write(&self, samples: &[i16]) {
        if let Ok(mut player) = self.player.lock() {
            if samples.is_empty() {
                return;
            }
            let mut stereo = Vec::with_capacity(samples.len() * 2);
            for sample in samples {
                let normalized = (*sample as f32) / (i16::MAX as f32);
                stereo.push(normalized);
                stereo.push(normalized);
            }
            player.push_samples(&stereo);
        }
    }
}

struct C64Frontend {
    video: Rc<RefCell<C64FrameBuffer>>,
    _sdl: sdl2::Sdl,
    canvas: Canvas<Window>,
    texture: Texture,
    event_pump: sdl2::EventPump,
    controller: ControllerManager,
    limit_fps: bool,
}

impl C64Frontend {
    fn new(video: Rc<RefCell<C64FrameBuffer>>, scale: u32, limit_fps: bool) -> Result<Self> {
        let sdl = sdl2::init().map_err(|err| anyhow!(err))?;
        let video_subsystem = sdl.video().map_err(|err| anyhow!(err))?;
        let frame = video.borrow();
        let width = (frame.width as u32).saturating_mul(scale);
        let height = (frame.height as u32).saturating_mul(scale);
        drop(frame);
        let window = video_subsystem
            .window("Commodore 64", width, height)
            .resizable()
            .position_centered()
            .build()
            .context("failed to create C64 window")?;
        let mut canvas_builder = window.into_canvas();
        if limit_fps {
            canvas_builder = canvas_builder.present_vsync();
        }
        let mut canvas = canvas_builder.build().map_err(|err| anyhow!(err))?;
        let (logical_w, logical_h) = {
            let fb = video.borrow();
            (fb.width as u32, fb.height as u32)
        };
        canvas
            .set_logical_size(logical_w, logical_h)
            .context("failed to set logical size")?;
        let texture_creator = canvas.texture_creator();
        let texture = texture_creator
            .create_texture_streaming(PixelFormatEnum::ARGB8888, logical_w, logical_h)
            .map_err(|err| anyhow!(err))?;
        let event_pump = sdl.event_pump().map_err(|err| anyhow!(err))?;
        let controller = ControllerManager::new(&sdl)?;
        Ok(Self {
            video,
            _sdl: sdl,
            canvas,
            texture,
            event_pump,
            controller,
            limit_fps,
        })
    }

    fn run(&mut self, c64: &mut C64) -> Result<()> {
        let mut running = true;
        while running {
            let frame_start = Instant::now();
            for event in self.event_pump.poll_iter() {
                self.controller.handle_event(&event);
                match event {
                    Event::Quit { .. } => running = false,
                    Event::KeyDown {
                        keycode: Some(code),
                        repeat,
                        ..
                    } => {
                        if !repeat {
                            handle_key_event(c64, code, true);
                        }
                    }
                    Event::KeyUp {
                        keycode: Some(code),
                        ..
                    } => {
                        handle_key_event(c64, code, false);
                    }
                    _ => {}
                }
            }
            if !running {
                break;
            }

            update_joystick(c64, &self.controller);
            c64.run_frame();
            self.present_frame();

            if self.limit_fps {
                let elapsed = frame_start.elapsed();
                if elapsed < FRAME_DURATION {
                    std::thread::sleep(FRAME_DURATION - elapsed);
                }
            }
        }
        Ok(())
    }

    fn present_frame(&mut self) {
        if let Ok(buffer) = self.video.try_borrow() {
            if self
                .texture
                .update(
                    None,
                    bytemuck::cast_slice(buffer.pixels()),
                    buffer.width * 4,
                )
                .is_ok()
            {
                let _ = self.canvas.copy(&self.texture, None, None);
                self.canvas.present();
            }
        }
    }
}

fn handle_key_event(c64: &mut C64, keycode: Keycode, pressed: bool) {
    if let Some(event) = map_keycode(keycode) {
        let keyboard = c64.get_keyboard();
        if pressed {
            keyboard.on_key_down(event);
        } else {
            keyboard.on_key_up(event);
        }
    }
}

fn map_keycode(keycode: Keycode) -> Option<KeyEvent> {
    use Key::*;
    match keycode {
        Keycode::A => Some(KeyEvent::new(A)),
        Keycode::B => Some(KeyEvent::new(B)),
        Keycode::C => Some(KeyEvent::new(C)),
        Keycode::D => Some(KeyEvent::new(D)),
        Keycode::E => Some(KeyEvent::new(E)),
        Keycode::F => Some(KeyEvent::new(F)),
        Keycode::G => Some(KeyEvent::new(G)),
        Keycode::H => Some(KeyEvent::new(H)),
        Keycode::I => Some(KeyEvent::new(I)),
        Keycode::J => Some(KeyEvent::new(J)),
        Keycode::K => Some(KeyEvent::new(K)),
        Keycode::L => Some(KeyEvent::new(L)),
        Keycode::M => Some(KeyEvent::new(M)),
        Keycode::N => Some(KeyEvent::new(N)),
        Keycode::O => Some(KeyEvent::new(O)),
        Keycode::P => Some(KeyEvent::new(P)),
        Keycode::Q => Some(KeyEvent::new(Q)),
        Keycode::R => Some(KeyEvent::new(R)),
        Keycode::S => Some(KeyEvent::new(S)),
        Keycode::T => Some(KeyEvent::new(T)),
        Keycode::U => Some(KeyEvent::new(U)),
        Keycode::V => Some(KeyEvent::new(V)),
        Keycode::W => Some(KeyEvent::new(W)),
        Keycode::X => Some(KeyEvent::new(X)),
        Keycode::Y => Some(KeyEvent::new(Y)),
        Keycode::Z => Some(KeyEvent::new(Z)),
        Keycode::Num0 => Some(KeyEvent::new(Num0)),
        Keycode::Num1 => Some(KeyEvent::new(Num1)),
        Keycode::Num2 => Some(KeyEvent::new(Num2)),
        Keycode::Num3 => Some(KeyEvent::new(Num3)),
        Keycode::Num4 => Some(KeyEvent::new(Num4)),
        Keycode::Num5 => Some(KeyEvent::new(Num5)),
        Keycode::Num6 => Some(KeyEvent::new(Num6)),
        Keycode::Num7 => Some(KeyEvent::new(Num7)),
        Keycode::Num8 => Some(KeyEvent::new(Num8)),
        Keycode::Num9 => Some(KeyEvent::new(Num9)),
        Keycode::Space => Some(KeyEvent::new(Space)),
        Keycode::Return | Keycode::KpEnter => Some(KeyEvent::new(Return)),
        Keycode::Backspace => Some(KeyEvent::new(Backspace)),
        Keycode::Escape => Some(KeyEvent::new(RunStop)),
        Keycode::Comma => Some(KeyEvent::new(Comma)),
        Keycode::Period => Some(KeyEvent::new(Period)),
        Keycode::Semicolon => Some(KeyEvent::new(Semicolon)),
        Keycode::Slash => Some(KeyEvent::new(Slash)),
        Keycode::Minus => Some(KeyEvent::new(Minus)),
        Keycode::Equals => Some(KeyEvent::new(Equals)),
        Keycode::LeftBracket => Some(KeyEvent::new(At)),
        Keycode::RightBracket => Some(KeyEvent::new(Colon)),
        Keycode::Quote => Some(KeyEvent::new(Plus)),
        Keycode::LShift => Some(KeyEvent::new(LShift)),
        Keycode::RShift => Some(KeyEvent::new(RShift)),
        Keycode::LCtrl | Keycode::RCtrl => Some(KeyEvent::new(Ctrl)),
        Keycode::Home => Some(KeyEvent::new(Home)),
        Keycode::F1 => Some(KeyEvent::new(F1)),
        Keycode::F3 => Some(KeyEvent::new(F3)),
        Keycode::F5 => Some(KeyEvent::new(F5)),
        Keycode::F7 => Some(KeyEvent::new(F7)),
        Keycode::Down => Some(KeyEvent::new(CrsrDown)),
        Keycode::Up => Some(KeyEvent::with_mod(CrsrDown, LShift)),
        Keycode::Right => Some(KeyEvent::new(CrsrRight)),
        Keycode::Left => Some(KeyEvent::with_mod(CrsrRight, LShift)),
        _ => None,
    }
}

fn update_joystick(c64: &mut C64, controllers: &ControllerManager) {
    if let Some(joy) = c64.get_joystick2_mut().as_mut() {
        let up = controllers.is_pressed(VirtualButton::Up);
        if up {
            joy.on_key_down(JoyButton::Up);
        } else {
            joy.on_key_up(JoyButton::Up);
        }
        let down = controllers.is_pressed(VirtualButton::Down);
        if down {
            joy.on_key_down(JoyButton::Down);
        } else {
            joy.on_key_up(JoyButton::Down);
        }
        let left = controllers.is_pressed(VirtualButton::Left);
        if left {
            joy.on_key_down(JoyButton::Left);
        } else {
            joy.on_key_up(JoyButton::Left);
        }
        let right = controllers.is_pressed(VirtualButton::Right);
        if right {
            joy.on_key_down(JoyButton::Right);
        } else {
            joy.on_key_up(JoyButton::Right);
        }
        let fire = controllers.is_pressed(VirtualButton::A)
            || controllers.is_pressed(VirtualButton::B)
            || controllers.is_pressed(VirtualButton::X)
            || controllers.is_pressed(VirtualButton::Y);
        if fire {
            joy.on_key_down(JoyButton::Fire);
        } else {
            joy.on_key_up(JoyButton::Fire);
        }
    }
}
