use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use desmume_rs::DeSmuME;
use desmume_rs::input::{Key, keymask};
use sdl2::event::{Event, WindowEvent};
use sdl2::keyboard::Keycode;
use sdl2::mouse::MouseButton;
use sdl2::pixels::PixelFormatEnum;
use sdl2::render::{Canvas, Texture};
use sdl2::video::Window;

const TARGET_FRAME: Duration = Duration::from_micros(16_667);
const SCREEN_WIDTH: u32 = desmume_rs::SCREEN_WIDTH as u32;
const SCREEN_HEIGHT: u32 = desmume_rs::SCREEN_HEIGHT as u32;
const SCREEN_HEIGHT_BOTH: u32 = desmume_rs::SCREEN_HEIGHT_BOTH as u32;

pub fn run(rom: &Path, scale: u32, limit_fps: bool) -> Result<()> {
    let title = rom
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Nintendo DS");
    let rom_path = rom
        .to_str()
        .ok_or_else(|| anyhow!("ROM path contains invalid UTF-8"))?;

    let mut nds = DeSmuME::init().map_err(|err| anyhow!(err))?;
    nds.open(rom_path, true).map_err(|err| anyhow!(err))?;

    let mut frontend = NdsFrontend::new(title, scale.max(1), limit_fps)?;
    frontend.run(&mut nds)
}

struct NdsFrontend {
    _sdl: sdl2::Sdl,
    canvas: Canvas<Window>,
    texture: Texture,
    event_pump: sdl2::EventPump,
    pressed: HashSet<Keycode>,
    limit_fps: bool,
    window_size: (u32, u32),
    touch_active: bool,
    pixel_buffer: Vec<u8>,
    argb_buffer: Vec<u32>,
}

impl NdsFrontend {
    fn new(title: &str, scale: u32, limit_fps: bool) -> Result<Self> {
        let sdl = sdl2::init().map_err(|e| anyhow!(e))?;
        let video = sdl.video().map_err(|e| anyhow!(e))?;
        let scaled_w = SCREEN_WIDTH.saturating_mul(scale);
        let scaled_h = SCREEN_HEIGHT_BOTH.saturating_mul(scale);
        let window = video
            .window(title, scaled_w, scaled_h)
            .position_centered()
            .resizable()
            .build()
            .context("failed to create NDS window")?;

        let mut canvas_builder = window.into_canvas();
        if limit_fps {
            canvas_builder = canvas_builder.present_vsync();
        }
        let mut canvas = canvas_builder.build().map_err(|e| anyhow!(e))?;
        canvas
            .set_logical_size(SCREEN_WIDTH, SCREEN_HEIGHT_BOTH)
            .context("failed to set NDS logical size")?;

        let texture_creator = canvas.texture_creator();
        let texture = texture_creator
            .create_texture_streaming(PixelFormatEnum::RGBA32, SCREEN_WIDTH, SCREEN_HEIGHT_BOTH)
            .map_err(|e| anyhow!(e))?;

        let event_pump = sdl.event_pump().map_err(|e| anyhow!(e))?;

        Ok(Self {
            _sdl: sdl,
            canvas,
            texture,
            event_pump,
            pressed: HashSet::new(),
            limit_fps,
            window_size: (scaled_w.max(1), scaled_h.max(1)),
            touch_active: false,
            pixel_buffer: vec![0; (SCREEN_WIDTH as usize) * (SCREEN_HEIGHT_BOTH as usize) * 4],
            argb_buffer: vec![0; (SCREEN_WIDTH as usize) * (SCREEN_HEIGHT_BOTH as usize)],
        })
    }

    fn run(&mut self, nds: &mut DeSmuME) -> Result<()> {
        let mut running = true;
        let mut last_frame = Instant::now();
        while running {
            while let Some(event) = self.event_pump.poll_event() {
                if !self.handle_event(nds, event) {
                    running = false;
                    break;
                }
            }

            if !running {
                break;
            }

            self.sync_inputs(nds);
            nds.cycle();
            self.present_frame(nds)?;

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

    fn handle_event(&mut self, nds: &mut DeSmuME, event: Event) -> bool {
        match event {
            Event::Quit { .. } => return false,
            Event::KeyDown {
                keycode: Some(Keycode::Escape),
                ..
            } => return false,
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
            Event::MouseButtonDown {
                mouse_btn: MouseButton::Left,
                x,
                y,
                ..
            } => {
                if let Some((tx, ty)) = self.touch_from_window(x, y) {
                    self.touch_active = true;
                    nds.input_mut().touch_set_pos(tx, ty);
                } else if self.touch_active {
                    self.touch_active = false;
                    nds.input_mut().touch_release();
                }
            }
            Event::MouseButtonUp {
                mouse_btn: MouseButton::Left,
                ..
            } => {
                if self.touch_active {
                    self.touch_active = false;
                    nds.input_mut().touch_release();
                }
            }
            Event::MouseMotion { x, y, .. } => {
                if self.touch_active {
                    if let Some((tx, ty)) = self.touch_from_window(x, y) {
                        nds.input_mut().touch_set_pos(tx, ty);
                    } else {
                        self.touch_active = false;
                        nds.input_mut().touch_release();
                    }
                }
            }
            Event::Window { win_event, .. } => match win_event {
                WindowEvent::SizeChanged(w, h) | WindowEvent::Resized(w, h) => {
                    self.window_size = (w.max(1) as u32, h.max(1) as u32);
                }
                WindowEvent::FocusLost | WindowEvent::Leave => {
                    if self.touch_active {
                        self.touch_active = false;
                        nds.input_mut().touch_release();
                    }
                }
                _ => {}
            },
            _ => {}
        }
        true
    }

    fn sync_inputs(&mut self, nds: &mut DeSmuME) {
        let mut mask = 0u16;
        for code in &self.pressed {
            if let Some(key) = map_keycode(*code) {
                mask |= keymask(key);
            }
        }
        nds.input_mut().keypad_update(mask);
    }

    fn present_frame(&mut self, nds: &DeSmuME) -> Result<()> {
        unsafe {
            nds.display_buffer_as_rgbx_into(&mut self.pixel_buffer);
        }

        for (dst, chunk) in self
            .argb_buffer
            .iter_mut()
            .zip(self.pixel_buffer.chunks_exact(4))
        {
            let r = chunk[2] as u32;
            let g = chunk[1] as u32;
            let b = chunk[0] as u32;
            *dst = (0xFF << 24) | (b << 16) | (g << 8) | r;
        }

        self.texture
            .update(
                None,
                bytemuck::cast_slice(&self.argb_buffer),
                (SCREEN_WIDTH as usize) * 4,
            )
            .context("failed to upload NDS frame")?;
        self.canvas.clear();
        self.canvas
            .copy(&self.texture, None, None)
            .map_err(|e| anyhow!(e))?;
        self.canvas.present();
        Ok(())
    }

    fn touch_from_window(&self, x: i32, y: i32) -> Option<(u16, u16)> {
        if x < 0 || y < 0 {
            return None;
        }
        let (win_w, win_h) = self.window_size;
        if win_w == 0 || win_h == 0 {
            return None;
        }

        let xf = (x as f32) / (win_w as f32);
        let yf = (y as f32) / (win_h as f32);
        if !(0.0..=1.0).contains(&xf) || !(0.0..=1.0).contains(&yf) {
            return None;
        }

        let ds_x = (xf * SCREEN_WIDTH as f32)
            .floor()
            .clamp(0.0, (SCREEN_WIDTH - 1) as f32);
        let ds_y_full = (yf * SCREEN_HEIGHT_BOTH as f32)
            .floor()
            .clamp(0.0, (SCREEN_HEIGHT_BOTH - 1) as f32);
        if ds_y_full < SCREEN_HEIGHT as f32 {
            return None;
        }
        let ds_y = (ds_y_full - SCREEN_HEIGHT as f32).clamp(0.0, (SCREEN_HEIGHT - 1) as f32);

        Some((ds_x as u16, ds_y as u16))
    }
}

fn map_keycode(code: Keycode) -> Option<Key> {
    match code {
        Keycode::X => Some(Key::A),
        Keycode::Z => Some(Key::B),
        Keycode::S => Some(Key::X),
        Keycode::A => Some(Key::Y),
        Keycode::Q => Some(Key::L),
        Keycode::W => Some(Key::R),
        Keycode::Return => Some(Key::Start),
        Keycode::Space | Keycode::Backspace | Keycode::RShift | Keycode::LShift => {
            Some(Key::Select)
        }
        Keycode::Up => Some(Key::Up),
        Keycode::Down => Some(Key::Down),
        Keycode::Left => Some(Key::Left),
        Keycode::Right => Some(Key::Right),
        _ => None,
    }
}
