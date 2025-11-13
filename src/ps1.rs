use std::{
    env, hint,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Error, Result, anyhow, bail};
use log::{info, warn};
use trapezoid_core::{DigitalControllerKey, Psx, PsxConfig, cpu::CpuState};
use vulkano::{
    Validated, VulkanError, VulkanLibrary,
    device::{
        Device, DeviceCreateInfo, DeviceExtensions, Queue, QueueCreateInfo, QueueFlags,
        physical::{PhysicalDevice, PhysicalDeviceType},
    },
    image::{Image, ImageUsage},
    instance::{Instance, InstanceCreateInfo},
    swapchain::{
        self, CompositeAlpha, PresentMode, Surface, Swapchain, SwapchainCreateInfo,
        SwapchainPresentInfo,
    },
    sync::{self, GpuFuture},
};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

use crate::{
    audio::AudioPlayer,
    controller::{ControllerManager, VirtualButton},
};

const TARGET_FPS: f64 = 59.5;
const DEFAULT_WIDTH: u32 = 640;
const DEFAULT_HEIGHT: u32 = 480;
const TRIGGER_THRESHOLD: f32 = 0.3;
const DIGITAL_KEY_COUNT: usize = DigitalControllerKey::Square as usize + 1;
const INPUT_SOURCE_COUNT: usize = 2;
const BIOS_CANDIDATE_NAMES: [&str; 4] = [
    "scph1001.bin",
    "SCPH1001.BIN",
    "scph5501.bin",
    "SCPH5501.BIN",
];
const DIGITAL_KEYS: [DigitalControllerKey; DIGITAL_KEY_COUNT] = [
    DigitalControllerKey::Select,
    DigitalControllerKey::L3,
    DigitalControllerKey::R3,
    DigitalControllerKey::Start,
    DigitalControllerKey::Up,
    DigitalControllerKey::Right,
    DigitalControllerKey::Down,
    DigitalControllerKey::Left,
    DigitalControllerKey::L2,
    DigitalControllerKey::R2,
    DigitalControllerKey::L1,
    DigitalControllerKey::R1,
    DigitalControllerKey::Triangle,
    DigitalControllerKey::Circle,
    DigitalControllerKey::X,
    DigitalControllerKey::Square,
];

pub fn run(
    rom_path: &Path,
    scale: u32,
    limit_fps: bool,
    bios_override: Option<PathBuf>,
) -> Result<()> {
    let bios_path = resolve_bios_path(bios_override)?;
    let rom_label = rom_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("PlayStation")
        .to_string();
    info!(
        "Booting PlayStation title {} with BIOS {}",
        rom_path.display(),
        bios_path.display()
    );

    let event_loop = EventLoop::new().context("failed to create PlayStation window event loop")?;
    let required_extensions = Surface::required_extensions(&event_loop)
        .context("failed to query required surface extensions")?;
    let library = VulkanLibrary::new().context("failed to load Vulkan loader")?;
    let instance = Instance::new(
        library,
        InstanceCreateInfo {
            enabled_extensions: required_extensions,
            ..Default::default()
        },
    )
    .context("failed to create Vulkan instance")?;

    let device_extensions = DeviceExtensions {
        khr_swapchain: true,
        ..DeviceExtensions::empty()
    };
    let (physical_device, queue_family_index) =
        select_physical_device(&instance, &event_loop, &device_extensions)
            .context("no Vulkan device can present to this display")?;

    let (device, mut queues) = Device::new(
        physical_device,
        DeviceCreateInfo {
            enabled_extensions: device_extensions,
            queue_create_infos: vec![QueueCreateInfo {
                queue_family_index,
                ..Default::default()
            }],
            ..Default::default()
        },
    )
    .context("failed to create logical device")?;
    let queue = queues
        .next()
        .ok_or_else(|| anyhow!("failed to acquire graphics queue"))?;

    let psx = Psx::new(
        &bios_path,
        Some(rom_path),
        PsxConfig {
            stdout_debug: false,
            fast_boot: true,
        },
        device.clone(),
        queue.clone(),
    )
    .map_err(|err| anyhow!(err))
    .with_context(|| format!("failed to boot {}", rom_path.display()))?;

    let audio = AudioPlayer::new().context("failed to initialize SDL2 audio for PlayStation")?;
    let controller =
        ControllerBridge::new().context("failed to initialize SDL2 controller subsystem")?;

    let mut app = Ps1App::new(
        instance,
        device,
        queue,
        psx,
        audio,
        controller,
        rom_label,
        scale.max(1),
        limit_fps,
    );

    event_loop
        .run_app(&mut app)
        .map_err(|err| anyhow!(err))
        .context("PlayStation window loop exited abnormally")?;

    if let Some(err) = app.shutdown_error.take() {
        return Err(err);
    }

    Ok(())
}

struct Ps1App {
    instance: Arc<Instance>,
    device: Arc<Device>,
    queue: Arc<Queue>,
    psx: Psx,
    audio: AudioPlayer,
    controller: ControllerBridge,
    render_context: Option<RenderContext>,
    frame_future: Option<Box<dyn GpuFuture>>,
    recreate_swapchain: bool,
    frame_timer: FrameTimer,
    limit_fps: bool,
    input_latch: InputLatch,
    rom_label: String,
    scale: u32,
    full_vram_display: bool,
    pub shutdown_error: Option<Error>,
}

impl Ps1App {
    fn new(
        instance: Arc<Instance>,
        device: Arc<Device>,
        queue: Arc<Queue>,
        psx: Psx,
        audio: AudioPlayer,
        controller: ControllerBridge,
        rom_label: String,
        scale: u32,
        limit_fps: bool,
    ) -> Self {
        Self {
            instance,
            device,
            queue,
            psx,
            audio,
            controller,
            render_context: None,
            frame_future: None,
            recreate_swapchain: false,
            frame_timer: FrameTimer::new(TARGET_FPS),
            limit_fps,
            input_latch: InputLatch::new(),
            rom_label,
            scale,
            full_vram_display: false,
            shutdown_error: None,
        }
    }

    fn handle_redraw_requested(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(err) = self.render_cycle() {
            self.shutdown_error = Some(err);
            event_loop.exit();
        }
    }

    fn render_cycle(&mut self) -> Result<()> {
        if self.render_context.is_none() {
            return Ok(());
        }
        self.frame_timer.enforce(self.limit_fps);
        self.controller
            .sync_inputs(&mut self.input_latch, &mut self.psx);

        let cpu_state = self.psx.clock_full_video_frame();
        if cpu_state != CpuState::Normal {
            warn!("PlayStation core reported state {:?}", cpu_state);
        }
        let samples = self.psx.take_audio_buffer();
        if !samples.is_empty() {
            self.audio.push_samples(&samples);
        }
        self.frame_timer.tick();
        self.render_frame()
    }

    fn render_frame(&mut self) -> Result<()> {
        let render = match self.render_context.as_mut() {
            Some(ctx) => ctx,
            None => return Ok(()),
        };
        let mut current_future = self
            .frame_future
            .take()
            .unwrap_or_else(|| sync::now(self.device.clone()).boxed());
        current_future.cleanup_finished();

        if self.recreate_swapchain {
            self.recreate_swapchain = false;
            let dimensions: [u32; 2] = render.window.inner_size().into();
            let (swapchain, images) = render
                .swapchain
                .recreate(SwapchainCreateInfo {
                    image_extent: dimensions,
                    ..render.swapchain.create_info()
                })
                .map_err(|err| anyhow!(err))
                .context("failed to recreate PlayStation swapchain")?;
            render.swapchain = swapchain;
            render.images = images;
        }

        let (image_index, suboptimal, acquire_future) =
            match swapchain::acquire_next_image(render.swapchain.clone(), None)
                .map_err(Validated::unwrap)
            {
                Ok(result) => result,
                Err(VulkanError::OutOfDate) => {
                    self.recreate_swapchain = true;
                    self.frame_future = Some(current_future);
                    return Ok(());
                }
                Err(err) => {
                    return Err(anyhow!(err).context("failed to acquire PS1 swapchain image"));
                }
            };

        if suboptimal {
            self.recreate_swapchain = true;
        }

        let image = render.images[image_index as usize].clone();
        let future = self.psx.blit_to_front(
            image,
            self.full_vram_display,
            current_future.join(acquire_future).boxed(),
        );
        let submitted = future
            .then_swapchain_present(
                self.queue.clone(),
                SwapchainPresentInfo::swapchain_image_index(render.swapchain.clone(), image_index),
            )
            .then_signal_fence_and_flush();

        self.frame_future = Some(match submitted {
            Ok(fut) => fut.boxed(),
            Err(Validated::Error(VulkanError::OutOfDate)) => {
                self.recreate_swapchain = true;
                sync::now(self.device.clone()).boxed()
            }
            Err(err) => return Err(anyhow!(err).context("failed to submit PS1 frame")),
        });

        let fps = self.frame_timer.fps();
        render.window.set_title(&format!(
            "PlayStation - {} ({:.1} FPS)",
            self.rom_label, fps
        ));
        Ok(())
    }

    fn init_window(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        if self.render_context.is_some() {
            return Ok(());
        }

        let width = DEFAULT_WIDTH.saturating_mul(self.scale.max(1));
        let height = DEFAULT_HEIGHT.saturating_mul(self.scale.max(1));
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title(format!("PlayStation - {}", self.rom_label))
                        .with_inner_size(PhysicalSize::new(width, height))
                        .with_resizable(true),
                )
                .context("failed to create PlayStation window")?,
        );
        let window_id = window.id();
        let surface = Surface::from_window(self.instance.clone(), window.clone())
            .context("failed to create Vulkan surface")?;

        let (swapchain, images) = {
            let caps = self
                .device
                .physical_device()
                .surface_capabilities(&surface, Default::default())
                .map_err(|err| anyhow!(err))
                .context("failed to query surface capabilities")?;
            let format = self
                .device
                .physical_device()
                .surface_formats(&surface, Default::default())
                .map_err(|err| anyhow!(err))
                .context("failed to query surface formats")?
                .first()
                .map(|entry| entry.0)
                .ok_or_else(|| anyhow!("surface reported no formats"))?;
            let present_mode = self
                .device
                .physical_device()
                .surface_present_modes(&surface, Default::default())
                .map_err(|err| anyhow!(err))
                .context("failed to query present modes")?
                .into_iter()
                .min_by_key(present_mode_rank)
                .unwrap_or(PresentMode::Fifo);
            let dimensions: [u32; 2] = window.inner_size().into();
            Swapchain::new(
                self.device.clone(),
                surface.clone(),
                SwapchainCreateInfo {
                    min_image_count: caps.min_image_count,
                    image_format: format,
                    image_extent: dimensions,
                    image_usage: ImageUsage::TRANSFER_DST,
                    composite_alpha: CompositeAlpha::Opaque,
                    present_mode,
                    ..Default::default()
                },
            )
            .map_err(|err| anyhow!(err))
            .context("failed to create PlayStation swapchain")?
        };

        self.render_context = Some(RenderContext {
            window,
            window_id,
            swapchain,
            images,
        });
        self.frame_future = Some(sync::now(self.device.clone()).boxed());
        self.recreate_swapchain = false;
        Ok(())
    }

    fn handle_keyboard_input(&mut self, event_loop: &ActiveEventLoop, event: &KeyEvent) {
        if event.repeat {
            return;
        }
        let pressed = event.state == ElementState::Pressed;
        if let PhysicalKey::Code(code) = event.physical_key {
            match code {
                KeyCode::Escape if pressed => {
                    event_loop.exit();
                    return;
                }
                KeyCode::KeyV if pressed => {
                    self.full_vram_display = !self.full_vram_display;
                }
                _ => {}
            }
            if let Some(mapped) = map_keyboard_key(code) {
                self.input_latch
                    .set(InputSource::Keyboard, mapped, pressed, &mut self.psx);
            }
        }
    }
}

impl ApplicationHandler for Ps1App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(err) = self.init_window(event_loop) {
            self.shutdown_error = Some(err);
            event_loop.exit();
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(ctx) = &self.render_context {
            ctx.window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.render_context.as_ref().map(|ctx| ctx.window_id) != Some(window_id) {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.handle_redraw_requested(event_loop),
            WindowEvent::KeyboardInput { event, .. } => {
                self.handle_keyboard_input(event_loop, &event);
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.recreate_swapchain = true;
            }
            WindowEvent::Focused(false) => {
                self.input_latch
                    .clear_source(InputSource::Keyboard, &mut self.psx);
            }
            _ => {}
        }
    }
}

struct RenderContext {
    window: Arc<Window>,
    window_id: WindowId,
    swapchain: Arc<Swapchain>,
    images: Vec<Arc<Image>>,
}

struct FrameTimer {
    target: f64,
    last_frame: Instant,
    average: MovingAverage,
}

impl FrameTimer {
    fn new(target: f64) -> Self {
        Self {
            target,
            last_frame: Instant::now(),
            average: MovingAverage::new(1.0 / target),
        }
    }

    fn enforce(&mut self, limit: bool) {
        if !limit {
            return;
        }
        let frame_duration = Duration::from_secs_f64(1.0 / self.target);
        loop {
            let elapsed = self.last_frame.elapsed();
            if elapsed >= frame_duration {
                break;
            }
            if frame_duration - elapsed > Duration::from_millis(1) {
                thread::sleep(frame_duration - elapsed - Duration::from_millis(1));
            } else {
                hint::spin_loop();
            }
        }
    }

    fn tick(&mut self) {
        let now = Instant::now();
        let delta = now.duration_since(self.last_frame).as_secs_f64();
        self.last_frame = now;
        self.average.push(delta);
    }

    fn fps(&self) -> f64 {
        1.0 / self.average.average()
    }
}

struct MovingAverage {
    values: [f64; 120],
    index: usize,
    sum: f64,
}

impl MovingAverage {
    fn new(initial: f64) -> Self {
        Self {
            values: [initial; 120],
            index: 0,
            sum: initial * 120.0,
        }
    }

    fn push(&mut self, value: f64) {
        self.sum -= self.values[self.index];
        self.values[self.index] = value;
        self.sum += value;
        self.index = (self.index + 1) % self.values.len();
    }

    fn average(&self) -> f64 {
        self.sum / self.values.len() as f64
    }
}

struct ControllerBridge {
    _sdl: sdl2::Sdl,
    manager: ControllerManager,
    event_pump: sdl2::EventPump,
}

impl ControllerBridge {
    fn new() -> Result<Self> {
        let sdl = sdl2::init().map_err(|err| anyhow!(err))?;
        let manager = ControllerManager::new(&sdl)?;
        let event_pump = sdl.event_pump().map_err(|err| anyhow!(err))?;
        Ok(Self {
            _sdl: sdl,
            manager,
            event_pump,
        })
    }

    fn sync_inputs(&mut self, latch: &mut InputLatch, psx: &mut Psx) {
        for event in self.event_pump.poll_iter() {
            self.manager.handle_event(&event);
        }

        latch.set(
            InputSource::Controller,
            DigitalControllerKey::Start,
            self.manager.is_pressed(VirtualButton::Start),
            psx,
        );
        latch.set(
            InputSource::Controller,
            DigitalControllerKey::Select,
            self.manager.is_pressed(VirtualButton::Select),
            psx,
        );
        latch.set(
            InputSource::Controller,
            DigitalControllerKey::Up,
            self.manager.is_pressed(VirtualButton::Up),
            psx,
        );
        latch.set(
            InputSource::Controller,
            DigitalControllerKey::Down,
            self.manager.is_pressed(VirtualButton::Down),
            psx,
        );
        latch.set(
            InputSource::Controller,
            DigitalControllerKey::Left,
            self.manager.is_pressed(VirtualButton::Left),
            psx,
        );
        latch.set(
            InputSource::Controller,
            DigitalControllerKey::Right,
            self.manager.is_pressed(VirtualButton::Right),
            psx,
        );
        latch.set(
            InputSource::Controller,
            DigitalControllerKey::X,
            self.manager.is_pressed(VirtualButton::A),
            psx,
        );
        latch.set(
            InputSource::Controller,
            DigitalControllerKey::Circle,
            self.manager.is_pressed(VirtualButton::B),
            psx,
        );
        latch.set(
            InputSource::Controller,
            DigitalControllerKey::Square,
            self.manager.is_pressed(VirtualButton::X),
            psx,
        );
        latch.set(
            InputSource::Controller,
            DigitalControllerKey::Triangle,
            self.manager.is_pressed(VirtualButton::Y),
            psx,
        );
        latch.set(
            InputSource::Controller,
            DigitalControllerKey::L1,
            self.manager.is_pressed(VirtualButton::L),
            psx,
        );
        latch.set(
            InputSource::Controller,
            DigitalControllerKey::R1,
            self.manager.is_pressed(VirtualButton::R),
            psx,
        );

        let analog = self.manager.analog_state().unwrap_or_default();
        latch.set(
            InputSource::Controller,
            DigitalControllerKey::L2,
            analog.left_trigger > TRIGGER_THRESHOLD,
            psx,
        );
        latch.set(
            InputSource::Controller,
            DigitalControllerKey::R2,
            analog.right_trigger > TRIGGER_THRESHOLD,
            psx,
        );
    }
}

#[derive(Clone, Copy)]
enum InputSource {
    Keyboard = 0,
    Controller = 1,
}

impl InputSource {
    fn idx(self) -> usize {
        self as usize
    }
}

struct InputLatch {
    sources: [[bool; DIGITAL_KEY_COUNT]; INPUT_SOURCE_COUNT],
    combined: [bool; DIGITAL_KEY_COUNT],
}

impl InputLatch {
    fn new() -> Self {
        Self {
            sources: [[false; DIGITAL_KEY_COUNT]; INPUT_SOURCE_COUNT],
            combined: [false; DIGITAL_KEY_COUNT],
        }
    }

    fn set(
        &mut self,
        source: InputSource,
        key: DigitalControllerKey,
        pressed: bool,
        psx: &mut Psx,
    ) {
        let idx = key as usize;
        self.sources[source.idx()][idx] = pressed;
        let next = self.sources.iter().any(|state| state[idx]);
        if next != self.combined[idx] {
            self.combined[idx] = next;
            psx.change_controller_key_state(key, next);
        }
    }

    fn clear_source(&mut self, source: InputSource, psx: &mut Psx) {
        for key in DIGITAL_KEYS {
            self.set(source, key, false, psx);
        }
    }
}

fn map_keyboard_key(code: KeyCode) -> Option<DigitalControllerKey> {
    match code {
        KeyCode::ArrowUp => Some(DigitalControllerKey::Up),
        KeyCode::ArrowDown => Some(DigitalControllerKey::Down),
        KeyCode::ArrowLeft => Some(DigitalControllerKey::Left),
        KeyCode::ArrowRight => Some(DigitalControllerKey::Right),
        KeyCode::KeyZ => Some(DigitalControllerKey::X),
        KeyCode::KeyX => Some(DigitalControllerKey::Circle),
        KeyCode::KeyA => Some(DigitalControllerKey::Square),
        KeyCode::KeyS => Some(DigitalControllerKey::Triangle),
        KeyCode::KeyQ => Some(DigitalControllerKey::L1),
        KeyCode::KeyW => Some(DigitalControllerKey::R1),
        KeyCode::Digit1 => Some(DigitalControllerKey::L2),
        KeyCode::Digit2 => Some(DigitalControllerKey::R2),
        KeyCode::Enter | KeyCode::NumpadEnter => Some(DigitalControllerKey::Start),
        KeyCode::Space | KeyCode::Backspace | KeyCode::ShiftLeft | KeyCode::ShiftRight => {
            Some(DigitalControllerKey::Select)
        }
        _ => None,
    }
}

fn present_mode_rank(mode: &PresentMode) -> u8 {
    match mode {
        PresentMode::Mailbox => 0,
        PresentMode::Immediate => 1,
        PresentMode::Fifo => 2,
        PresentMode::FifoRelaxed => 3,
        _ => 4,
    }
}

fn resolve_bios_path(bios_override: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = bios_override {
        return validate_existing_file(path);
    }
    for key in ["PS1_BIOS", "PSX_BIOS"] {
        if let Ok(value) = env::var(key) {
            return validate_existing_file(PathBuf::from(&value))
                .with_context(|| format!("{} points to {}", key, value));
        }
    }

    for candidate in default_bios_candidates() {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    bail!(
        "PlayStation BIOS not found. Pass --ps1-bios, set PS1_BIOS/PSX_BIOS, or place SCPH1001/SCPH5501 under bios/ps1/."
    );
}

fn default_bios_candidates() -> Vec<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut bases = vec![
        PathBuf::from("bios/ps1"),
        PathBuf::from("bios"),
        PathBuf::from("."),
    ];
    bases.extend([
        manifest_dir.join("bios/ps1"),
        manifest_dir.join("bios"),
        manifest_dir.clone(),
    ]);

    let mut paths = Vec::with_capacity(BIOS_CANDIDATE_NAMES.len() * bases.len());
    for base in bases {
        for name in BIOS_CANDIDATE_NAMES {
            paths.push(base.join(name));
        }
    }
    paths
}

fn validate_existing_file(path: PathBuf) -> Result<PathBuf> {
    if path.is_file() {
        Ok(path)
    } else {
        bail!("PlayStation BIOS file {} does not exist", path.display())
    }
}

fn select_physical_device(
    instance: &Arc<Instance>,
    event_loop: &EventLoop<()>,
    extensions: &DeviceExtensions,
) -> Result<(Arc<PhysicalDevice>, u32)> {
    instance
        .enumerate_physical_devices()
        .map_err(|err| anyhow!(err))
        .context("failed to enumerate physical devices")?
        .into_iter()
        .filter(|device| device.supported_extensions().contains(extensions))
        .filter_map(|device| {
            let queue_index = device
                .queue_family_properties()
                .iter()
                .enumerate()
                .find(|(idx, info)| {
                    info.queue_flags.contains(QueueFlags::GRAPHICS)
                        && device
                            .presentation_support(*idx as u32, &event_loop)
                            .unwrap_or(false)
                })
                .map(|(idx, _)| idx as u32)?;
            Some((device, queue_index))
        })
        .min_by_key(|(device, _)| match device.properties().device_type {
            PhysicalDeviceType::DiscreteGpu => 0,
            PhysicalDeviceType::IntegratedGpu => 1,
            PhysicalDeviceType::VirtualGpu => 2,
            PhysicalDeviceType::Cpu => 3,
            _ => 4,
        })
        .ok_or_else(|| anyhow!("no Vulkan device with graphics+present support was found"))
}
