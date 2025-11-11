use std::collections::HashSet;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use gameboy_core::Gameboy;
use gameboy_core::button::Button;
use gameboy_core::emulator::step_result::StepResult;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::PixelFormatEnum;
use sdl2::render::{Canvas, Texture};
use sdl2::video::Window;

use crate::ai::AiController;
use crate::audio::AudioPlayer;
use crate::automation::AutomationRecorder;
use crate::display::{FrameBuffer, HEIGHT, WIDTH};

const TARGET_FRAME: Duration = Duration::from_micros(16_667);

pub struct InteractiveRunner {
    _sdl: sdl2::Sdl,
    canvas: Canvas<Window>,
    texture: Texture,
    event_pump: sdl2::EventPump,
    framebuffer: FrameBuffer,
    pressed: HashSet<Button>,
    limit_fps: bool,
    frame_counter: u64,
}

impl InteractiveRunner {
    pub fn new(title: &str, scale: u32, limit_fps: bool) -> Result<Self> {
        let sdl = sdl2::init().map_err(|e| anyhow!(e))?;
        let video = sdl.video().map_err(|e| anyhow!(e))?;
        let scaled_w = (WIDTH as u32).saturating_mul(scale.max(1));
        let scaled_h = (HEIGHT as u32).saturating_mul(scale.max(1));
        let window = video
            .window(title, scaled_w, scaled_h)
            .position_centered()
            .resizable()
            .build()
            .context("failed to create SDL window")?;

        let mut canvas_builder = window.into_canvas();
        if limit_fps {
            canvas_builder = canvas_builder.present_vsync();
        }
        let mut canvas = canvas_builder.build().map_err(|e| anyhow!(e))?;
        canvas
            .set_logical_size(WIDTH as u32, HEIGHT as u32)
            .context("failed to set logical size")?;

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
            framebuffer: FrameBuffer::new(),
            pressed: HashSet::new(),
            limit_fps,
            frame_counter: 0,
        })
    }

    pub fn run(
        &mut self,
        gameboy: &mut Gameboy,
        audio: &mut AudioPlayer,
        mut recorder: Option<&mut AutomationRecorder>,
        mut ai: Option<&mut AiController>,
    ) -> Result<()> {
        let mut running = true;
        let mut last_frame = Instant::now();
        while running {
            let events: Vec<_> = self.event_pump.poll_iter().collect();
            for event in events {
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
                    } => self.handle_press(gameboy, code),
                    Event::KeyUp {
                        keycode: Some(code),
                        ..
                    } => self.handle_release(gameboy, code),
                    _ => {}
                }
            }

            if let Some(controller) = ai.as_deref_mut() {
                controller.tick(gameboy)?;
            }

            self.emulate_frame(gameboy, audio, recorder.as_deref_mut())?;
            self.present_frame()?;

            if self.limit_fps {
                let elapsed = last_frame.elapsed();
                if elapsed < TARGET_FRAME {
                    std::thread::sleep(TARGET_FRAME - elapsed);
                }
                last_frame = Instant::now();
            }
        }
        if let Some(controller) = ai {
            controller.stop(gameboy);
        }
        Ok(())
    }

    fn handle_press(&mut self, gameboy: &mut Gameboy, code: Keycode) {
        if let Some(button) = map_key(code) {
            if self.pressed.insert(button) {
                gameboy.press_button(button);
            }
        }
    }

    fn handle_release(&mut self, gameboy: &mut Gameboy, code: Keycode) {
        if let Some(button) = map_key(code) {
            if self.pressed.remove(&button) {
                gameboy.release_button(button);
            }
        }
    }

    fn emulate_frame(
        &mut self,
        gameboy: &mut Gameboy,
        audio: &mut AudioPlayer,
        recorder: Option<&mut AutomationRecorder>,
    ) -> Result<()> {
        loop {
            match gameboy.emulate(&mut self.framebuffer) {
                StepResult::VBlank => {
                    if let Some(rec) = recorder {
                        rec.record(self.frame_counter, gameboy)?;
                    }
                    self.frame_counter += 1;
                    break;
                }
                StepResult::AudioBufferFull => audio.push_samples(gameboy.get_audio_buffer()),
                StepResult::Nothing => {}
            };
        }
        Ok(())
    }

    fn present_frame(&mut self) -> Result<()> {
        self.texture
            .update(None, self.framebuffer.as_bytes(), WIDTH * 4)
            .context("failed to upload frame")?;
        self.canvas.clear();
        self.canvas
            .copy(&self.texture, None, None)
            .map_err(|e| anyhow!(e))?;
        self.canvas.present();
        Ok(())
    }
}

fn map_key(key: Keycode) -> Option<Button> {
    match key {
        Keycode::Left => Some(Button::Left),
        Keycode::Right => Some(Button::Right),
        Keycode::Up => Some(Button::Up),
        Keycode::Down => Some(Button::Down),
        Keycode::Z => Some(Button::A),
        Keycode::X => Some(Button::B),
        Keycode::Return => Some(Button::Start),
        Keycode::RShift | Keycode::LShift | Keycode::Space | Keycode::Backspace => {
            Some(Button::Select)
        }
        _ => None,
    }
}
