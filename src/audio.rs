use anyhow::{Result, anyhow};
use sdl2::audio::{AudioQueue, AudioSpecDesired};

pub struct AudioPlayer {
    _sdl: sdl2::Sdl,
    queue: AudioQueue<f32>,
}

impl AudioPlayer {
    pub fn new() -> Result<Self> {
        let sdl = sdl2::init().map_err(|e| anyhow!(e))?;
        let audio = sdl.audio().map_err(|e| anyhow!(e))?;
        let desired = AudioSpecDesired {
            freq: Some(44_100),
            channels: Some(2),
            samples: Some(1024),
        };
        let queue = audio
            .open_queue::<f32, _>(None, &desired)
            .map_err(|e| anyhow!(e))?;
        queue.resume();
        Ok(Self { _sdl: sdl, queue })
    }

    pub fn push_samples(&mut self, samples: &[f32]) {
        if let Err(err) = self.queue.queue_audio(samples) {
            eprintln!("Audio queue error: {err}");
        }
        // if we build up more than ~1 second of buffered audio, drop it to keep latency sane
        const MAX_BUFFER_BYTES: u32 = 44_100 * 2 * 4; // ~1 second of stereo f32
        if self.queue.size() > MAX_BUFFER_BYTES {
            self.queue.clear();
        }
    }

}
