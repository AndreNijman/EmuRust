use gameboy_core::{Button, Gameboy};

/// Placeholder ML controller that will eventually learn actions.
/// For now it simply exposes the plumbing required to drive the emulator.
pub struct AiController {
    held: Option<Button>,
}

impl AiController {
    pub fn new() -> Self {
        Self { held: None }
    }

    /// Called each iteration to update the desired input state.
    pub fn tick(&mut self, gameboy: &mut Gameboy) {
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

    fn decide(&mut self, _gameboy: &Gameboy) -> Option<Button> {
        // Placeholder: no AI yet. Returning None keeps inputs neutral.
        None
    }
}
