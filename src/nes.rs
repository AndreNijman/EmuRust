use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use gc_nes_core::cartridge::Cartridge;
use gc_nes_core::nes::{NES_SCREEN_DIMENSIONS, Nes};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::PixelFormatEnum;
use sdl2::render::{Canvas, Texture};
use sdl2::video::Window;

const WIDTH: usize = 256;
const HEIGHT: usize = 240;
const TARGET_FRAME: Duration = Duration::from_micros(16_667);

pub fn run(rom: &Path, scale: u32, limit_fps: bool) -> Result<()> {
    let cartridge = Cartridge::load_from_file(rom)
        .map_err(|err| anyhow!("failed to load NES ROM {}: {err}", rom.display()))?;
    let mut nes = Nes::new(cartridge);
    let title = rom.file_stem().and_then(|s| s.to_str()).unwrap_or("NES");

    let mut frontend = NesFrontend::new(title, scale.max(1), limit_fps)?;
    frontend.run(&mut nes)
}

struct NesFrontend {
    _sdl: sdl2::Sdl,
    canvas: Canvas<Window>,
    texture: Texture,
    event_pump: sdl2::EventPump,
    pressed: HashSet<Keycode>,
    limit_fps: bool,
    argb_buffer: Vec<u32>,
}

impl NesFrontend {
    fn new(title: &str, scale: u32, limit_fps: bool) -> Result<Self> {
        let sdl = sdl2::init().map_err(|e| anyhow!(e))?;
        let video = sdl.video().map_err(|e| anyhow!(e))?;
        let scaled_w = (WIDTH as u32).saturating_mul(scale.max(1));
        let scaled_h = (HEIGHT as u32).saturating_mul(scale.max(1));
        let window = video
            .window(title, scaled_w, scaled_h)
            .position_centered()
            .resizable()
            .build()
            .context("failed to create NES window")?;

        let mut canvas_builder = window.into_canvas();
        if limit_fps {
            canvas_builder = canvas_builder.present_vsync();
        }
        let mut canvas = canvas_builder.build().map_err(|e| anyhow!(e))?;
        canvas
            .set_logical_size(WIDTH as u32, HEIGHT as u32)
            .context("failed to set NES logical size")?;

        let texture_creator = canvas.texture_creator();
        let texture = texture_creator
            .create_texture_streaming(PixelFormatEnum::ARGB8888, WIDTH as u32, HEIGHT as u32)
            .map_err(|e| anyhow!(e))?;
        let event_pump = sdl.event_pump().map_err(|e| anyhow!(e))?;

        Ok(Self {
            _sdl: sdl,
            canvas,
            texture,
            event_pump,
            pressed: HashSet::new(),
            limit_fps,
            argb_buffer: vec![0; NES_SCREEN_DIMENSIONS],
        })
    }

    fn run(&mut self, nes: &mut Nes) -> Result<()> {
        let mut running = true;
        let mut last_frame = Instant::now();
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

            nes.update_controller_one(Some(controller_state(&self.pressed)));
            let frame = nes.frame();
            self.present_frame(frame)?;

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

    fn present_frame(&mut self, pixels: &[u32; NES_SCREEN_DIMENSIONS]) -> Result<()> {
        for (dst, src) in self.argb_buffer.iter_mut().zip(pixels.iter()) {
            *dst = 0xFF00_0000 | *src;
        }
        self.texture
            .update(None, bytemuck::cast_slice(&self.argb_buffer), WIDTH * 4)
            .context("failed to upload NES frame")?;
        self.canvas.clear();
        self.canvas
            .copy(&self.texture, None, None)
            .map_err(|e| anyhow!(e))?;
        self.canvas.present();
        Ok(())
    }
}

fn controller_state(pressed: &HashSet<Keycode>) -> u8 {
    let mut state = 0u8;
    if pressed.contains(&Keycode::X) {
        state |= 0b0000_0001;
    }
    if pressed.contains(&Keycode::Z) {
        state |= 0b0000_0010;
    }
    if pressed.contains(&Keycode::Space)
        || pressed.contains(&Keycode::Backspace)
        || pressed.contains(&Keycode::RShift)
        || pressed.contains(&Keycode::LShift)
    {
        state |= 0b0000_0100;
    }
    if pressed.contains(&Keycode::Return) {
        state |= 0b0000_1000;
    }
    if pressed.contains(&Keycode::Up) {
        state |= 0b0001_0000;
    }
    if pressed.contains(&Keycode::Down) {
        state |= 0b0010_0000;
    }
    if pressed.contains(&Keycode::Left) {
        state |= 0b0100_0000;
    }
    if pressed.contains(&Keycode::Right) {
        state |= 0b1000_0000;
    }
    state
}
