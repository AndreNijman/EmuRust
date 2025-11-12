use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow};
use sdl2::GameControllerSubsystem;
use sdl2::controller::{Axis, Button as SdlButton, GameController};
use sdl2::event::Event;

const STICK_DEADZONE: i16 = 9_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VirtualButton {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    X,
    Y,
    L,
    R,
    Start,
    Select,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ControllerAnalogState {
    pub left_x: f32,
    pub left_y: f32,
    pub right_x: f32,
    pub right_y: f32,
    pub left_trigger: f32,
    pub right_trigger: f32,
}

pub struct ControllerManager {
    subsystem: GameControllerSubsystem,
    controllers: HashMap<u32, ControllerDevice>,
    pressed_counts: HashMap<VirtualButton, u32>,
}

struct ControllerDevice {
    _controller: GameController,
    buttons: HashSet<VirtualButton>,
    axis_buttons: HashSet<VirtualButton>,
    left_x: i16,
    left_y: i16,
    right_x: i16,
    right_y: i16,
    left_trigger: i16,
    right_trigger: i16,
}

impl ControllerManager {
    pub fn new(sdl: &sdl2::Sdl) -> Result<Self> {
        let subsystem = sdl.game_controller().map_err(|err| anyhow!(err))?;
        let mut manager = Self {
            subsystem,
            controllers: HashMap::new(),
            pressed_counts: HashMap::new(),
        };
        manager.scan_existing()?;
        Ok(manager)
    }

    pub fn handle_event(&mut self, event: &Event) {
        match *event {
            Event::ControllerDeviceAdded { which, .. } => {
                let _ = self.open_controller(which);
            }
            Event::ControllerDeviceRemoved { which, .. } => {
                self.remove_controller(which);
            }
            Event::ControllerButtonDown { which, button, .. } => {
                if let Some(vb) = map_button(button) {
                    self.set_button_state(which, vb, true, false);
                }
            }
            Event::ControllerButtonUp { which, button, .. } => {
                if let Some(vb) = map_button(button) {
                    self.set_button_state(which, vb, false, false);
                }
            }
            Event::ControllerAxisMotion {
                which, axis, value, ..
            } => {
                self.handle_axis(which, axis, value);
            }
            _ => {}
        }
    }

    pub fn is_pressed(&self, button: VirtualButton) -> bool {
        self.pressed_counts
            .get(&button)
            .copied()
            .unwrap_or_default()
            > 0
    }

    pub fn analog_state(&self) -> Option<ControllerAnalogState> {
        self.controllers
            .values()
            .next()
            .map(|device| ControllerAnalogState {
                left_x: normalize_axis(device.left_x),
                left_y: normalize_axis(device.left_y),
                right_x: normalize_axis(device.right_x),
                right_y: normalize_axis(device.right_y),
                left_trigger: normalize_trigger(device.left_trigger),
                right_trigger: normalize_trigger(device.right_trigger),
            })
    }

    fn scan_existing(&mut self) -> Result<()> {
        let num = self.subsystem.num_joysticks().map_err(|err| anyhow!(err))? as u32;
        for idx in 0..num {
            let _ = self.open_controller(idx);
        }
        Ok(())
    }

    fn open_controller(&mut self, device_index: u32) -> Result<()> {
        if !self.subsystem.is_game_controller(device_index) {
            return Ok(());
        }
        let controller = self
            .subsystem
            .open(device_index)
            .map_err(|err| anyhow!(err))?;
        let instance_id = controller.instance_id();
        self.controllers.insert(
            instance_id,
            ControllerDevice {
                _controller: controller,
                buttons: HashSet::new(),
                axis_buttons: HashSet::new(),
                left_x: 0,
                left_y: 0,
                right_x: 0,
                right_y: 0,
                left_trigger: 0,
                right_trigger: 0,
            },
        );
        Ok(())
    }

    fn remove_controller(&mut self, instance_id: u32) {
        if let Some(device) = self.controllers.remove(&instance_id) {
            for button in device.buttons.union(&device.axis_buttons) {
                self.decrement(*button);
            }
        }
    }

    fn set_button_state(
        &mut self,
        instance_id: u32,
        button: VirtualButton,
        pressed: bool,
        axis: bool,
    ) {
        if let Some(device) = self.controllers.get_mut(&instance_id) {
            let set = if axis {
                &mut device.axis_buttons
            } else {
                &mut device.buttons
            };
            let changed = if pressed {
                set.insert(button)
            } else {
                set.remove(&button)
            };
            if changed {
                if pressed {
                    *self.pressed_counts.entry(button).or_default() += 1;
                } else {
                    self.decrement(button);
                }
            }
        }
    }

    fn decrement(&mut self, button: VirtualButton) {
        if let Some(count) = self.pressed_counts.get_mut(&button) {
            if *count > 1 {
                *count -= 1;
            } else {
                self.pressed_counts.remove(&button);
            }
        }
    }

    fn handle_axis(&mut self, instance_id: u32, axis: Axis, value: i16) {
        let (old_buttons, target) = {
            let device = match self.controllers.get_mut(&instance_id) {
                Some(dev) => dev,
                None => return,
            };
            match axis {
                Axis::LeftX => device.left_x = value,
                Axis::LeftY => device.left_y = value,
                Axis::RightX => {
                    device.right_x = value;
                    return;
                }
                Axis::RightY => {
                    device.right_y = value;
                    return;
                }
                Axis::TriggerLeft => {
                    device.left_trigger = value;
                    return;
                }
                Axis::TriggerRight => {
                    device.right_trigger = value;
                    return;
                }
            }
            let target = axis_to_buttons(device.left_x, device.left_y);
            let old = device.axis_buttons.clone();
            device.axis_buttons = target.clone();
            (old, target)
        };
        for btn in old_buttons.difference(&target) {
            self.set_button_state(instance_id, *btn, false, true);
        }
        for btn in target.difference(&old_buttons) {
            self.set_button_state(instance_id, *btn, true, true);
        }
    }
}

fn axis_to_buttons(x: i16, y: i16) -> HashSet<VirtualButton> {
    let mut set = HashSet::new();
    if x <= -STICK_DEADZONE {
        set.insert(VirtualButton::Left);
    } else if x >= STICK_DEADZONE {
        set.insert(VirtualButton::Right);
    }
    if y <= -STICK_DEADZONE {
        set.insert(VirtualButton::Up);
    } else if y >= STICK_DEADZONE {
        set.insert(VirtualButton::Down);
    }
    set
}

fn normalize_axis(value: i16) -> f32 {
    let normalized = (value as f32) / (i16::MAX as f32);
    if normalized.abs() < 0.05 {
        0.0
    } else {
        normalized.clamp(-1.0, 1.0)
    }
}

fn normalize_trigger(value: i16) -> f32 {
    if value <= 0 {
        0.0
    } else {
        (value as f32 / i16::MAX as f32).clamp(0.0, 1.0)
    }
}

fn map_button(button: SdlButton) -> Option<VirtualButton> {
    match button {
        SdlButton::A => Some(VirtualButton::A),
        SdlButton::B => Some(VirtualButton::B),
        SdlButton::X => Some(VirtualButton::X),
        SdlButton::Y => Some(VirtualButton::Y),
        SdlButton::Back => Some(VirtualButton::Select),
        SdlButton::Start => Some(VirtualButton::Start),
        SdlButton::Guide => Some(VirtualButton::Start),
        SdlButton::LeftShoulder => Some(VirtualButton::L),
        SdlButton::RightShoulder => Some(VirtualButton::R),
        SdlButton::DPadUp => Some(VirtualButton::Up),
        SdlButton::DPadDown => Some(VirtualButton::Down),
        SdlButton::DPadLeft => Some(VirtualButton::Left),
        SdlButton::DPadRight => Some(VirtualButton::Right),
        _ => None,
    }
}
