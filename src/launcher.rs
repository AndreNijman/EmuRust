use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use log::warn;

use crate::systems::{GameSystem, detect_system};

const SUPPORTED_EXTENSIONS: [&str; 24] = [
    "gb", "gbc", "nes", "sfc", "smc", "snes", "nds", "cue", "exe", "n64", "z64", "v64", "iso",
    "gcm", "gcz", "gcn", "ciso", "dol", "rvz", "prg", "p00", "bin", "crt", "tap",
];

#[derive(Clone)]
struct GameEntry {
    path: PathBuf,
    name: String,
}

#[derive(Clone)]
struct SystemGroup {
    system: GameSystem,
    games: Vec<GameEntry>,
}

pub fn select_game(dir: &Path, use_gui: bool) -> Result<PathBuf> {
    let systems = collect_games(dir)?;
    if use_gui {
        gui::select_game_gui(systems)
    } else {
        select_game_tui(systems)
    }
}

fn collect_games(dir: &Path) -> Result<Vec<SystemGroup>> {
    let mut games_by_system: BTreeMap<GameSystem, Vec<GameEntry>> = BTreeMap::new();

    fs::read_dir(dir)
        .with_context(|| format!("failed to read games directory at {}", dir.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .for_each(|path| {
            if !path.is_file() {
                return;
            }
            let ext = match path.extension().and_then(|s| s.to_str()) {
                Some(ext) => ext.to_ascii_lowercase(),
                None => return,
            };
            if !SUPPORTED_EXTENSIONS.contains(&ext.as_str()) {
                return;
            }
            match detect_system(&path) {
                Ok(system) => {
                    games_by_system
                        .entry(system)
                        .or_default()
                        .push(GameEntry::from(path));
                }
                Err(err) => warn!("Skipping {}: {}", path.display(), err),
            };
        });

    if games_by_system.is_empty() {
        bail!("no compatible ROMs found under {}", dir.display());
    }

    let mut systems: Vec<SystemGroup> = games_by_system
        .into_iter()
        .map(|(system, mut games)| {
            games.sort_by(|a, b| a.name.cmp(&b.name));
            SystemGroup { system, games }
        })
        .collect();
    systems.sort_by_key(|group| group.system.label());

    Ok(systems)
}

fn select_game_tui(systems: Vec<SystemGroup>) -> Result<PathBuf> {
    loop {
        println!("\n=== Game Launcher ===");
        for (idx, group) in systems.iter().enumerate() {
            println!(
                "{:>2}. {} ({} game{})",
                idx + 1,
                group.system,
                group.games.len(),
                if group.games.len() == 1 { "" } else { "s" }
            );
        }

        let console_choice = prompt_number("Select a console by number: ", 1, systems.len()) - 1;

        let group = &systems[console_choice];
        println!("\n-- {} --", group.system);
        for (idx, entry) in group.games.iter().enumerate() {
            println!("{:>2}. {}", idx + 1, entry.name);
        }
        println!(" 0. Back to console list");

        let game_choice = prompt_number("Select a game (0 to go back): ", 0, group.games.len());
        if game_choice == 0 {
            continue;
        }

        return Ok(group.games[game_choice - 1].path.clone());
    }
}

fn prompt_number(prompt: &str, min: usize, max: usize) -> usize {
    loop {
        print!("{prompt}");
        io::stdout().flush().ok();
        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            println!("Failed to read input. Please try again.");
            continue;
        }
        if let Ok(choice) = input.trim().parse::<usize>() {
            if choice >= min && choice <= max {
                return choice;
            }
        }
        println!(
            "Invalid selection. Please enter a number between {} and {}.",
            min, max
        );
    }
}

impl GameEntry {
    fn from(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("<unknown>")
            .to_string();
        Self { path, name }
    }
}

mod gui {
    use super::{GameEntry, SystemGroup};
    use anyhow::{Context, Result, anyhow, bail};
    use bytemuck::cast_slice;
    use font8x8::legacy::BASIC_LEGACY;
    use sdl2::event::Event;
    use sdl2::keyboard::Keycode;
    use sdl2::mouse::MouseButton;
    use sdl2::pixels::PixelFormatEnum;
    use sdl2::render::{Canvas, Texture};
    use sdl2::video::Window;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    const WIDTH: u32 = 960;
    const HEIGHT: u32 = 640;
    const PADDING: usize = 24;
    const PANEL_GAP: usize = 24;
    const FOOTER_HEIGHT: usize = 54;
    const HEADER_GAP: usize = 34;
    const ROW_HEIGHT: usize = 28;
    const CONSOLE_PANEL_WIDTH: usize = 280;
    const DOUBLE_CLICK_TIMEOUT: Duration = Duration::from_millis(350);
    const FRAME_DELAY: Duration = Duration::from_millis(16);

    const BG_COLOR: u32 = 0xFF11121B;
    const PANEL_BORDER: u32 = 0xFF2A2D3F;
    const PANEL_COLOR: u32 = 0xFF1B1E2C;
    const TEXT_COLOR: u32 = 0xFFE4E8F6;
    const MUTED_TEXT: u32 = 0xFF9AA2BF;
    const HIGHLIGHT_ACTIVE: u32 = 0xFF3B82F6;
    const HIGHLIGHT_INACTIVE: u32 = 0xFF2C354E;
    const FOOTER_TEXT: u32 = 0xFF8F97B3;

    pub(super) fn select_game_gui(systems: Vec<SystemGroup>) -> Result<PathBuf> {
        let mut selector = GuiSelector::new(systems)?;
        selector.run()
    }

    struct GuiSelector {
        systems: Vec<SystemGroup>,
        active_column: Column,
        system_index: usize,
        game_index: usize,
        system_scroll: usize,
        game_scroll: usize,
        frame_buffer: Vec<u32>,
        _sdl: sdl2::Sdl,
        canvas: Canvas<Window>,
        texture: Texture,
        event_pump: sdl2::EventPump,
        last_click: Option<ClickInfo>,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Column {
        Systems,
        Games,
    }

    struct ClickInfo {
        index: usize,
        instant: Instant,
    }

    #[derive(Clone, Copy)]
    struct Panel {
        x: usize,
        y: usize,
        width: usize,
        height: usize,
    }

    struct Layout {
        consoles: Panel,
        games: Panel,
        visible_rows: usize,
        footer_top: usize,
    }

    impl GuiSelector {
        fn new(systems: Vec<SystemGroup>) -> Result<Self> {
            let sdl = sdl2::init().map_err(|err| anyhow!(err))?;
            let video = sdl.video().map_err(|err| anyhow!(err))?;
            let window = video
                .window("Retro Launcher", WIDTH, HEIGHT)
                .position_centered()
                .resizable()
                .build()
                .context("failed to create launcher window")?;
            let canvas = window
                .into_canvas()
                .present_vsync()
                .build()
                .map_err(|err| anyhow!(err))?;
            let texture_creator = canvas.texture_creator();
            let texture = texture_creator
                .create_texture_streaming(PixelFormatEnum::ARGB8888, WIDTH, HEIGHT)
                .map_err(|err| anyhow!(err))?;
            let event_pump = sdl.event_pump().map_err(|err| anyhow!(err))?;

            Ok(Self {
                systems,
                active_column: Column::Systems,
                system_index: 0,
                game_index: 0,
                system_scroll: 0,
                game_scroll: 0,
                frame_buffer: vec![BG_COLOR; (WIDTH * HEIGHT) as usize],
                _sdl: sdl,
                canvas,
                texture,
                event_pump,
                last_click: None,
            })
        }

        fn run(&mut self) -> Result<PathBuf> {
            loop {
                if let Some(selection) = self.poll_events()? {
                    return Ok(selection);
                }
                self.ensure_visibility();
                self.render()?;
                std::thread::sleep(FRAME_DELAY);
            }
        }

        fn poll_events(&mut self) -> Result<Option<PathBuf>> {
            let events: Vec<_> = self.event_pump.poll_iter().collect();
            for event in events {
                match event {
                    Event::Quit { .. } => bail!("game selection cancelled"),
                    Event::KeyDown {
                        keycode: Some(Keycode::Escape),
                        ..
                    } => bail!("game selection cancelled"),
                    Event::KeyDown {
                        keycode: Some(key),
                        repeat: false,
                        ..
                    } => {
                        if let Some(path) = self.handle_key(key)? {
                            return Ok(Some(path));
                        }
                    }
                    Event::MouseButtonDown {
                        mouse_btn, x, y, ..
                    } => {
                        if let Some(path) = self.handle_click(mouse_btn, x, y) {
                            return Ok(Some(path));
                        }
                    }
                    _ => {}
                }
            }
            Ok(None)
        }

        fn handle_key(&mut self, key: Keycode) -> Result<Option<PathBuf>> {
            match key {
                Keycode::Up => self.move_selection(-1),
                Keycode::Down => self.move_selection(1),
                Keycode::PageUp => self.page_move(-1),
                Keycode::PageDown => self.page_move(1),
                Keycode::Home => self.move_to_edge(false),
                Keycode::End => self.move_to_edge(true),
                Keycode::Left => self.active_column = Column::Systems,
                Keycode::Right => self.activate_games_column(),
                Keycode::Tab => {
                    if matches!(self.active_column, Column::Systems) {
                        self.activate_games_column();
                    } else {
                        self.active_column = Column::Systems;
                    }
                }
                Keycode::Return | Keycode::KpEnter => {
                    if matches!(self.active_column, Column::Systems) {
                        self.activate_games_column();
                    } else if let Some(path) = self.current_game_path() {
                        return Ok(Some(path));
                    }
                }
                _ => {}
            }
            Ok(None)
        }

        fn move_selection(&mut self, delta: isize) {
            match self.active_column {
                Column::Systems => {
                    let len = self.systems.len();
                    if len == 0 {
                        return;
                    }
                    let current = self.system_index as isize;
                    let next = (current + delta).clamp(0, (len - 1) as isize) as usize;
                    if next != self.system_index {
                        self.system_index = next;
                        self.game_index = 0;
                        self.game_scroll = 0;
                        self.last_click = None;
                    }
                }
                Column::Games => {
                    let len = self.current_games().len();
                    if len == 0 {
                        return;
                    }
                    let current = self.game_index as isize;
                    let next = (current + delta).clamp(0, (len - 1) as isize) as usize;
                    self.game_index = next;
                }
            }
        }

        fn page_move(&mut self, delta: isize) {
            let layout = self.layout();
            let step = layout.visible_rows.max(1) as isize;
            match self.active_column {
                Column::Systems => {
                    let len = self.systems.len();
                    if len == 0 {
                        return;
                    }
                    let current = self.system_index as isize;
                    let next = (current + step * delta).clamp(0, (len - 1) as isize) as usize;
                    if next != self.system_index {
                        self.system_index = next;
                        self.game_index = 0;
                        self.game_scroll = 0;
                        self.last_click = None;
                    }
                }
                Column::Games => {
                    let len = self.current_games().len();
                    if len == 0 {
                        return;
                    }
                    let current = self.game_index as isize;
                    let next = (current + step * delta).clamp(0, (len - 1) as isize) as usize;
                    self.game_index = next;
                }
            }
        }

        fn move_to_edge(&mut self, end: bool) {
            match self.active_column {
                Column::Systems => {
                    let len = self.systems.len();
                    if len == 0 {
                        return;
                    }
                    let target = if end { len - 1 } else { 0 };
                    if target != self.system_index {
                        self.system_index = target;
                        self.game_index = 0;
                        self.game_scroll = 0;
                        self.last_click = None;
                    }
                }
                Column::Games => {
                    let len = self.current_games().len();
                    if len == 0 {
                        return;
                    }
                    self.game_index = if end { len - 1 } else { 0 };
                }
            }
        }

        fn activate_games_column(&mut self) {
            if !self.current_games().is_empty() {
                self.active_column = Column::Games;
            }
        }

        fn handle_click(&mut self, button: MouseButton, x: i32, y: i32) -> Option<PathBuf> {
            if button != MouseButton::Left || x < 0 || y < 0 {
                return None;
            }
            let layout = self.layout();
            let point = (x as usize, y as usize);
            if layout.consoles.contains(point) {
                if let Some(idx) = layout.row_index(point.1, layout.consoles) {
                    let absolute =
                        (idx + self.system_scroll).min(self.systems.len().saturating_sub(1));
                    if absolute != self.system_index {
                        self.system_index = absolute;
                        self.game_index = 0;
                        self.game_scroll = 0;
                    }
                    self.active_column = Column::Systems;
                    self.last_click = None;
                }
            } else if layout.games.contains(point) {
                if self.current_games().is_empty() {
                    return None;
                }
                if let Some(idx) = layout.row_index(point.1, layout.games) {
                    let absolute =
                        (idx + self.game_scroll).min(self.current_games().len().saturating_sub(1));
                    self.game_index = absolute;
                    self.active_column = Column::Games;
                    if let Some(path) = self.register_click(absolute) {
                        return Some(path);
                    }
                }
            }
            None
        }

        fn register_click(&mut self, index: usize) -> Option<PathBuf> {
            let now = Instant::now();
            if let Some(prev) = &self.last_click {
                if prev.index == index && now.duration_since(prev.instant) <= DOUBLE_CLICK_TIMEOUT {
                    return self.current_game_path();
                }
            }
            self.last_click = Some(ClickInfo {
                index,
                instant: now,
            });
            None
        }

        fn current_games(&self) -> &[GameEntry] {
            &self.systems[self.system_index].games
        }

        fn current_game_path(&self) -> Option<PathBuf> {
            self.current_games()
                .get(self.game_index)
                .map(|entry| entry.path.clone())
        }

        fn ensure_visibility(&mut self) {
            let layout = self.layout();
            self.system_scroll = clamp_scroll(
                self.system_scroll,
                self.system_index,
                self.systems.len(),
                layout.visible_rows,
            );
            let game_len = self.current_games().len();
            if game_len > 0 {
                self.game_scroll = clamp_scroll(
                    self.game_scroll,
                    self.game_index,
                    game_len,
                    layout.visible_rows,
                );
            } else {
                self.game_scroll = 0;
                self.game_index = 0;
            }
        }

        fn layout(&self) -> Layout {
            let width = WIDTH as usize;
            let height = HEIGHT as usize;
            let panel_height = height
                .saturating_sub(PADDING * 2 + FOOTER_HEIGHT)
                .max(ROW_HEIGHT + HEADER_GAP);
            let consoles = Panel {
                x: PADDING,
                y: PADDING,
                width: CONSOLE_PANEL_WIDTH,
                height: panel_height,
            };
            let games_x = consoles.x + consoles.width + PANEL_GAP;
            let games_width = width
                .saturating_sub(games_x + PADDING)
                .max(CONSOLE_PANEL_WIDTH);
            let games = Panel {
                x: games_x,
                y: PADDING,
                width: games_width,
                height: panel_height,
            };
            let visible_rows = panel_height.saturating_sub(HEADER_GAP).max(ROW_HEIGHT) / ROW_HEIGHT;
            let footer_top = height.saturating_sub(FOOTER_HEIGHT - 12);
            Layout {
                consoles,
                games,
                visible_rows: visible_rows.max(1),
                footer_top,
            }
        }

        fn render(&mut self) -> Result<()> {
            self.frame_buffer.fill(BG_COLOR);
            let layout = self.layout();
            self.draw_panel(&layout.consoles);
            self.draw_panel(&layout.games);

            self.draw_text(
                layout.consoles.x + 12,
                layout.consoles.y + 10,
                "Consoles",
                MUTED_TEXT,
            );
            self.draw_text(
                layout.games.x + 12,
                layout.games.y + 10,
                self.systems[self.system_index].system.label(),
                MUTED_TEXT,
            );

            self.draw_system_rows(&layout);
            self.draw_game_rows(&layout);
            self.draw_footer(layout.footer_top);
            self.present_frame()
        }

        fn draw_panel(&mut self, panel: &Panel) {
            self.fill_rect(
                panel.x.saturating_sub(1),
                panel.y.saturating_sub(1),
                panel.width + 2,
                panel.height + 2,
                PANEL_BORDER,
            );
            self.fill_rect(panel.x, panel.y, panel.width, panel.height, PANEL_COLOR);
        }

        fn draw_system_rows(&mut self, layout: &Layout) {
            let is_active = matches!(self.active_column, Column::Systems);
            let visible = layout.visible_rows;
            for row in 0..visible {
                let idx = self.system_scroll + row;
                if idx >= self.systems.len() {
                    break;
                }
                let y = layout.consoles.y + HEADER_GAP + row * ROW_HEIGHT;
                let selected = idx == self.system_index;
                if selected {
                    let color = if is_active {
                        HIGHLIGHT_ACTIVE
                    } else {
                        HIGHLIGHT_INACTIVE
                    };
                    self.fill_rect(
                        layout.consoles.x + 6,
                        y,
                        layout.consoles.width - 12,
                        ROW_HEIGHT - 4,
                        color,
                    );
                }
                let group = &self.systems[idx];
                let title = format!("{} ({})", group.system.label(), group.games.len());
                let text_color = if selected {
                    if is_active { TEXT_COLOR } else { 0xFFE0E6FF }
                } else {
                    TEXT_COLOR
                };
                self.draw_text(layout.consoles.x + 14, y + 6, &title, text_color);
            }
        }

        fn draw_game_rows(&mut self, layout: &Layout) {
            let game_len = self.systems[self.system_index].games.len();
            if game_len == 0 {
                self.draw_text(
                    layout.games.x + 14,
                    layout.games.y + HEADER_GAP,
                    "No games found for this console.",
                    MUTED_TEXT,
                );
                return;
            }
            let is_active = matches!(self.active_column, Column::Games);
            let visible = layout.visible_rows;
            for row in 0..visible {
                let idx = self.game_scroll + row;
                if idx >= game_len {
                    break;
                }
                let y = layout.games.y + HEADER_GAP + row * ROW_HEIGHT;
                let selected = idx == self.game_index;
                if selected {
                    let color = if is_active {
                        HIGHLIGHT_ACTIVE
                    } else {
                        HIGHLIGHT_INACTIVE
                    };
                    self.fill_rect(
                        layout.games.x + 6,
                        y,
                        layout.games.width - 12,
                        ROW_HEIGHT - 4,
                        color,
                    );
                }
                let name = self.systems[self.system_index].games[idx].name.clone();
                let text_color = if selected && is_active {
                    TEXT_COLOR
                } else if selected {
                    0xFFE0E6FF
                } else {
                    TEXT_COLOR
                };
                self.draw_text(layout.games.x + 14, y + 6, &name, text_color);
            }
        }

        fn draw_footer(&mut self, y: usize) {
            let instructions =
                "Arrow keys navigate, Enter launches, Esc cancels, double-click a game to launch.";
            self.draw_text(PADDING, y, instructions, FOOTER_TEXT);
        }

        fn fill_rect(
            &mut self,
            start_x: usize,
            start_y: usize,
            width: usize,
            height: usize,
            color: u32,
        ) {
            let frame_width = WIDTH as usize;
            let frame_height = HEIGHT as usize;
            let max_y = (start_y + height).min(frame_height);
            let max_x = (start_x + width).min(frame_width);
            for y in start_y..max_y {
                let row_offset = y * frame_width;
                for x in start_x..max_x {
                    self.frame_buffer[row_offset + x] = color;
                }
            }
        }

        fn draw_text(&mut self, start_x: usize, start_y: usize, text: &str, color: u32) {
            let mut x = start_x;
            let mut y = start_y;
            for ch in text.chars() {
                if ch == '\n' {
                    y += 10;
                    x = start_x;
                    continue;
                }
                self.draw_char(x, y, ch, color);
                x += 8;
            }
        }

        fn draw_char(&mut self, start_x: usize, start_y: usize, ch: char, color: u32) {
            let glyph = glyph_for(ch);
            let frame_width = WIDTH as usize;
            let frame_height = HEIGHT as usize;
            for (row, row_bits) in glyph.iter().enumerate() {
                let y = start_y + row;
                if y >= frame_height {
                    break;
                }
                for col in 0..8 {
                    if (row_bits >> col) & 1 == 0 {
                        continue;
                    }
                    let x = start_x + col;
                    if x >= frame_width {
                        continue;
                    }
                    self.frame_buffer[y * frame_width + x] = color;
                }
            }
        }

        fn present_frame(&mut self) -> Result<()> {
            self.texture
                .update(
                    None,
                    cast_slice(&self.frame_buffer),
                    (WIDTH as usize * 4) as usize,
                )
                .map_err(|err| anyhow!(err))?;
            self.canvas.clear();
            self.canvas
                .copy(&self.texture, None, None)
                .map_err(|err| anyhow!(err))?;
            self.canvas.present();
            Ok(())
        }
    }

    impl Panel {
        fn contains(&self, point: (usize, usize)) -> bool {
            let (x, y) = point;
            x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
        }
    }

    impl Layout {
        fn row_index(&self, y: usize, panel: Panel) -> Option<usize> {
            if y < panel.y + HEADER_GAP || y >= panel.y + panel.height {
                return None;
            }
            let row = (y - panel.y - HEADER_GAP) / ROW_HEIGHT;
            Some(row)
        }
    }

    fn clamp_scroll(mut scroll: usize, index: usize, total: usize, visible: usize) -> usize {
        if total <= visible {
            return 0;
        }
        if index < scroll {
            scroll = index;
        } else {
            let bottom_threshold = scroll + visible - 1;
            if index > bottom_threshold {
                scroll = index + 1 - visible;
            }
        }
        scroll.min(total - visible)
    }

    fn glyph_for(ch: char) -> [u8; 8] {
        let idx = ch as usize;
        if idx < BASIC_LEGACY.len() {
            BASIC_LEGACY[idx]
        } else {
            BASIC_LEGACY['?' as usize]
        }
    }
}
