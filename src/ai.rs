use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use gameboy_core::{Button, Gameboy};
use log::info;
use rand::SeedableRng;
use rand::rngs::SmallRng;
use serde::Serialize;

const BOARD_ADDR: u16 = 0xC0A0; // 10x20 playfield in Game Boy Tetris
const BOARD_LEN: usize = 200;
const LEVEL_ADDR: u16 = 0xC200;
const LINES_ADDR: u16 = 0xC201;
const NEXT_PIECE_ADDR: u16 = 0xC203;

#[derive(Clone, Debug)]
pub struct AiConfig {
    pub seed: Option<u64>,
    pub log_path: Option<PathBuf>,
}

#[derive(Clone, Debug)]
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

#[derive(Serialize, Clone, Copy)]
pub enum AiAction {
    None,
    Left,
    Right,
    Down,
    Start,
}

impl AiAction {
    fn button(self) -> Option<Button> {
        match self {
            AiAction::None => None,
            AiAction::Left => Some(Button::Left),
            AiAction::Right => Some(Button::Right),
            AiAction::Down => Some(Button::Down),
            AiAction::Start => Some(Button::Start),
        }
    }
}

pub struct AiController {
    held: Option<Button>,
    frame_count: u64,
    last_logged: u64,
    latest: Option<GameObservation>,
    rng: SmallRng,
    logger: Option<BufWriter<File>>,
    start_counter: u8,
}

impl AiController {
    pub fn new(config: AiConfig) -> anyhow::Result<Self> {
        let seed = config.seed.unwrap_or(0xA17E115);
        let logger = if let Some(path) = config.log_path {
            Some(BufWriter::new(File::create(path)?))
        } else {
            None
        };
        Ok(Self {
            held: None,
            frame_count: 0,
            last_logged: 0,
            latest: None,
            rng: SmallRng::seed_from_u64(seed),
            logger,
            start_counter: 4,
        })
    }

    /// Called each iteration to update the desired input state.
    pub fn tick(&mut self, gameboy: &mut Gameboy) -> anyhow::Result<()> {
        self.frame_count += 1;
        let observation = capture_observation(gameboy, self.frame_count);
        self.maybe_log_summary(&observation);

        let action = if self.start_counter > 0 {
            self.start_counter -= 1;
            AiAction::Start
        } else {
            self.decide()
        };
        self.apply_action(gameboy, action);
        self.write_dataset_entry(&observation, action)?;
        self.latest = Some(observation);
        Ok(())
    }

    /// Release any held button (useful when exiting).
    pub fn stop(&mut self, gameboy: &mut Gameboy) {
        if let Some(prev) = self.held.take() {
            gameboy.release_button(prev);
        }
        if let Some(writer) = self.logger.as_mut() {
            let _ = writer.flush();
        }
    }

    pub fn latest_observation(&self) -> Option<&GameObservation> {
        self.latest.as_ref()
    }

    fn decide(&mut self) -> AiAction {
        let roll: f32 = rand::Rng::r#gen(&mut self.rng);
        match roll {
            r if r < 0.2 => AiAction::Left,
            r if r < 0.4 => AiAction::Right,
            r if r < 0.6 => AiAction::Down,
            _ => AiAction::None,
        }
    }

    fn apply_action(&mut self, gameboy: &mut Gameboy, action: AiAction) {
        let desired = action.button();
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

    fn maybe_log_summary(&mut self, obs: &GameObservation) {
        if obs.frame - self.last_logged >= 60 {
            info!(
                "AI obs frame {}: level {}, lines {}, cells {}, next {}",
                obs.frame,
                obs.level,
                obs.lines,
                obs.filled_cells(),
                obs.next_piece
            );
            self.last_logged = obs.frame;
        }
    }

    fn write_dataset_entry(
        &mut self,
        obs: &GameObservation,
        action: AiAction,
    ) -> anyhow::Result<()> {
        if let Some(writer) = self.logger.as_mut() {
            let json = serde_json::to_string(&LoggedSample::from(obs, action))?;
            writeln!(writer, "{}", json)?;
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct LoggedSample {
    frame: u64,
    level: u8,
    lines: u8,
    next_piece: u8,
    filled_cells: usize,
    action: AiAction,
    board_hex: String,
}

impl LoggedSample {
    fn from(obs: &GameObservation, action: AiAction) -> Self {
        Self {
            frame: obs.frame,
            level: obs.level,
            lines: obs.lines,
            next_piece: obs.next_piece,
            filled_cells: obs.filled_cells(),
            action,
            board_hex: encode_hex(&obs.board),
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

fn encode_hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{:02X}", b)).collect()
}
