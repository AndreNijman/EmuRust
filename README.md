# Retro Launcher (Game Boy / NES / SNES / NDS)

This project is a lightweight retro game launcher with built-in Game Boy, NES, SNES, and Nintendo DS
emulator cores (all rendered with SDL2). Drop ROMs into the `games/` directory and run one command
to pick a title—no external emulators required.

## Requirements

- Rust 1.76+
- SDL2 development libraries (`libsdl2-dev`, `SDL2-devel`, or `brew install sdl2`)
## Setup

```bash
cargo build --release
```

1. Place your ROMs under `games/` (a sample `tetris.gb` is already there).
2. Just drop your ROMs (`.gb`, `.gbc`, `.nes`, `.sfc`, `.smc`, `.snes`, `.nds`) into `games/`.

## Usage

Launch the menu:

```bash
cargo run --release
```

The launcher scans `games/`, shows a numbered list, and runs whichever entry you pick. To run a ROM
directly, skip the menu:

```bash
cargo run --release -- --rom games/tetris.gb
```

> **Frame pacing**: Windows default to ~60 FPS so titles run at their intended speed. Pass
> `--limit-fps=false` only if you explicitly want uncapped rendering (useful for debugging).

## Controller Support

Any SDL2-compatible gamepad works across every built-in core. The d-pad (or left stick) maps to the
console d-pads, `A/B/X/Y` map to the respective face buttons, shoulder buttons map to L/R, `Start`
maps to START, and `Back/Select` maps to SELECT. Nintendo DS titles still require a mouse for the
touchscreen, but all handheld/console buttons can be driven from the controller.

### Controls (Game Boy built-in core)

- Arrow keys: D-pad
- `Z`: A button
- `X`: B button
- `Enter`: START
- `Right Shift` (or `Space`/`Backspace`): SELECT
- `Esc` / window close: exit

### Controls (NES core)

- Arrow keys: D-pad
- `X`: A button
- `Z`: B button
- `Enter`: START
- `Right Shift` (or `Space`/`Backspace`): SELECT

### Controls (SNES core)

- Arrow keys: D-pad
- `X`: A
- `Z`: B
- `S`: X
- `A`: Y
- `Q`: L
- `W`: R
- `Enter`: START
- `Right Shift` (or `Space`/`Backspace`): SELECT

SNES battery-backed saves are written to a `.sav` file alongside the ROM.

### Controls (Nintendo DS core)

- Arrow keys: D-pad
- `X`: A
- `Z`: B
- `S`: X
- `A`: Y
- `Q`: L
- `W`: R
- `Enter`: START
- `Right Shift` (or `Left Shift` / `Space` / `Backspace`): SELECT
- Hold the left mouse button on the bottom screen to use the touchscreen stylus

## Folder Layout

```
games/
  tetris.gb         # sample Game Boy ROM
src/
  ...
```

Add more ROMs (GB/GBC/NES/SNES/NDS) to the `games/` folder and rerun the launcher whenever you want to
play.
