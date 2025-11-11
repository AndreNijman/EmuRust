use std::time::{SystemTime, UNIX_EPOCH};

use gameboy_core::emulator::traits::RTC;

pub struct SystemRtc;

impl RTC for SystemRtc {
    fn get_current_time(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}
