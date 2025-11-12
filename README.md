# Retro Launcher (Game Boy / NES / SNES / NDS / GameCube)

This project is a lightweight retro game launcher with built-in Game Boy, NES, SNES, Nintendo DS, and GameCube
emulator cores (all rendered with SDL2). Drop ROMs into the `games/` directory and run one command
to pick a title—no external emulators required.

## Requirements

- Rust 1.76+
- SDL2 development libraries (`libsdl2-dev`, `SDL2-devel`, or `brew install sdl2`)
- Dolphin emulator binary (optional, for full GameCube emulation—set `DOLPHIN_BIN` or place Dolphin on your `PATH`)
## Setup

```bash
cargo build --release
```

1. Place your ROMs under `games/` (a sample `tetris.gb` is already there).
2. Just drop your ROMs (`.gb`, `.gbc`, `.nes`, `.sfc`, `.smc`, `.snes`, `.nds`, `.iso`, `.gcm`, `.gcz`, `.gcn`, `.rvz`) into `games/`.

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

GameCube titles additionally parse the disc header on load—the launcher prints the game ID, maker,
disc, and streaming flags to the console and an overlay in the top-left corner of the window mirrors
that metadata so you can quickly confirm which image you booted. Both raw ISOs and Dolphin-style
`.rvz` images are supported—the latter are transparently decompressed in-memory at launch. If a
Dolphin binary is available (either via the `DOLPHIN_BIN` environment variable or on `PATH` under
names such as `dolphin-emu`), the launcher automatically hands the game off to Dolphin for full
emulation; otherwise it falls back to the built-in visualization stub so you still get metadata and
input plumbing even without the external core installed.

### Installing Dolphin on Arch Linux

```bash
sudo pacman -S dolphin-emu
export DOLPHIN_BIN="$(command -v dolphin-emu-nogui || command -v dolphin-emu)"
```

Leave the `DOLPHIN_BIN` export in your shell profile (or launch the game via `DOLPHIN_BIN=... cargo run --release`)
so the launcher knows which Dolphin binary to spawn. Any other platform can do the same as long as
the executable is either on `PATH` or the absolute path is assigned to `DOLPHIN_BIN`.

### Core Matrix

| System   | Core delivery                                                                                    |
|----------|--------------------------------------------------------------------------------------------------|
| Game Boy | Built-in Rust core (`gameboy_core` patched in-tree)                                              |
| NES      | Built-in Rust core (`gc_nes_core`)                                                               |
| SNES     | Built-in Rust core (`super-sabicom` via `meru-interface`)                                        |
| Nintendo DS | Built-in Rust core (`desmume-rs`)                                                             |
| GameCube | **External** Dolphin binary (auto-detected via `DOLPHIN_BIN` or `PATH`); stub visualization runs only when Dolphin is missing |

Only the first four systems compile into the launcher. GameCube requires the external Dolphin install described above in order to play games; without it you’ll just see the metadata overlay and placeholder graphics.

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

### Controls (GameCube core)

- Arrow keys / Controller d-pad or left stick: main stick + D-pad
- `X`: A, `Z`: B, `S`: X, `A`: Y
- `E` or either Shift key / controller Back-Select: Z
- `Q`: L, `W`: R (digital shoulders)
- `U` / `O`: analog L / R trigger sweep (controllers use their triggers automatically)
- `J` / `L` / `I` / `K`: C-stick (right stick)
- `Enter`: START
- `Esc` / window close: exit

## Folder Layout

```
games/
  tetris.gb         # sample Game Boy ROM
src/
  ...
```

Add more ROMs (GB/GBC/NES/SNES/NDS) to the `games/` folder and rerun the launcher whenever you want to
play.
