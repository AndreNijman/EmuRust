use bytemuck::cast_slice;
use gameboy_core::emulator::traits::PixelMapper;
use gameboy_core::{CGBColor, Color};

pub const WIDTH: usize = 160;
pub const HEIGHT: usize = 144;
const PIXELS: usize = WIDTH * HEIGHT;

const DMG_PALETTE: [u32; 4] = [
    rgb_to_u32(224, 248, 208),
    rgb_to_u32(136, 192, 112),
    rgb_to_u32(52, 104, 86),
    rgb_to_u32(8, 24, 32),
];

const fn rgb_to_u32(r: u8, g: u8, b: u8) -> u32 {
    ((0xFFu32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

pub struct FrameBuffer {
    pixels: Vec<u32>,
}

impl FrameBuffer {
    pub fn new() -> Self {
        Self {
            pixels: vec![DMG_PALETTE[0]; PIXELS],
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        cast_slice(&self.pixels)
    }
}

impl PixelMapper for FrameBuffer {
    fn map_pixel(&mut self, pixel: usize, color: Color) {
        if pixel < self.pixels.len() {
            self.pixels[pixel] = DMG_PALETTE[color_index(color)];
        }
    }

    fn cgb_map_pixel(&mut self, pixel: usize, color: CGBColor) {
        if pixel < self.pixels.len() {
            self.pixels[pixel] = rgb_to_u32(color.red, color.green, color.blue);
        }
    }
}

fn color_index(color: Color) -> usize {
    match color {
        Color::White => 0,
        Color::LightGray => 1,
        Color::DarkGray => 2,
        Color::Black => 3,
    }
}
