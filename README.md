# Retro Launcher (Game Boy / NES / SNES)

This project is a lightweight retro game launcher with built-in Game Boy, NES, and SNES emulator
cores (all rendered with SDL2). Drop ROMs into the `games/` directory and run one command to pick a
title—no external emulators required.

## Requirements

- Rust 1.76+
- SDL2 development libraries (`libsdl2-dev`, `SDL2-devel`, or `brew install sdl2`)
## Setup

```bash
cargo build --release
```

1. Place your ROMs under `games/` (a sample `tetris.gb` is already there).
2. Just drop your ROMs (`.gb`, `.gbc`, `.nes`, `.sfc`, `.smc`, `.snes`) into `games/`.

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

## Folder Layout

```
games/
  tetris.gb         # sample Game Boy ROM
src/
  ...
```

Add more ROMs (GB/GBC/NES/SNES) to the `games/` folder and rerun the launcher whenever you want to
play.
