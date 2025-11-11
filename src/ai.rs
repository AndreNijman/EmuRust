use gameboy_core::{Button, Gameboy};
use log::info;

const BOARD_ADDR: u16 = 0xC0A0; // 10x20 playfield in Game Boy Tetris
const BOARD_LEN: usize = 200;
const LEVEL_ADDR: u16 = 0xC200;
const LINES_ADDR: u16 = 0xC201;
const NEXT_PIECE_ADDR: u16 = 0xC203;

/// Snapshot of the relevant memory for downstream ML models.
pub struct GameObservation {
    pub frame: u64,
    pub board: [u8; BOARD_LEN],
    pub level: u8,
    pub lines: u8,
    pub next_piece: u8,
}

impl GameObservation {
    pub fn filled_cells(&self) -> usize {
        self.board.iter().filter(|cell| **cell != 0).count()
    }
}

pub struct AiController {
    held: Option<Button>,
    frame_count: u64,
    last_logged: u64,
    latest: Option<GameObservation>,
}

impl AiController {
    pub fn new() -> Self {
        Self {
            held: None,
            frame_count: 0,
            last_logged: 0,
            latest: None,
        }
    }

    /// Called each iteration to update the desired input state.
    pub fn tick(&mut self, gameboy: &mut Gameboy) {
        self.frame_count += 1;
        let observation = capture_observation(gameboy, self.frame_count);
        self.maybe_log(&observation);
        self.latest = Some(observation);

        let desired = self.decide(gameboy);
        if desired == self.held {
            return;
        }

        if let Some(prev) = self.held {
            gameboy.release_button(prev);
        }
        if let Some(next) = desired {
            gameboy.press_button(next);
        }
        self.held = desired;
    }

    /// Release any held button (useful when exiting).
    pub fn stop(&mut self, gameboy: &mut Gameboy) {
        if let Some(prev) = self.held.take() {
            gameboy.release_button(prev);
        }
    }

    pub fn latest_observation(&self) -> Option<&GameObservation> {
        self.latest.as_ref()
    }

    fn decide(&mut self, _gameboy: &Gameboy) -> Option<Button> {
        // Placeholder: no AI yet. Returning None keeps inputs neutral.
        None
    }

    fn maybe_log(&mut self, obs: &GameObservation) {
        if obs.frame - self.last_logged >= 60 {
            info!(
                "AI observation frame {}: level {}, lines {}, filled cells {}",
                obs.frame,
                obs.level,
                obs.lines,
                obs.filled_cells()
            );
            self.last_logged = obs.frame;
        }
    }
}

fn capture_observation(gameboy: &Gameboy, frame: u64) -> GameObservation {
    let mut board = [0u8; BOARD_LEN];
    gameboy.peek_block(BOARD_ADDR, &mut board);
    let level = gameboy.peek_byte(LEVEL_ADDR);
    let lines = gameboy.peek_byte(LINES_ADDR);
    let next_piece = gameboy.peek_byte(NEXT_PIECE_ADDR);

    GameObservation {
        frame,
        board,
        level,
        lines,
        next_piece,
    }
}
