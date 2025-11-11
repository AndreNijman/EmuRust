# Retro Launcher (Game Boy / NES / SNES)

This project now behaves as a lightweight game launcher. Drop ROMs into the `games/` directory and
run one command to pick a title. `.gb/.gbc` ROMs run via the built-in Game Boy emulator (SDL2 video
+ SDL2 audio). `.nes` and `.sfc/.smc/.snes` files are forwarded to external emulators so you can
reuse the same key bindings you already configured elsewhere.

## Requirements

- Rust 1.76+
- SDL2 development libraries (`libsdl2-dev`, `SDL2-devel`, or `brew install sdl2`)
- Optional: external emulator binaries referenced by the env vars below

| Env var       | Purpose                                                     |
|---------------|-------------------------------------------------------------|
| `NES_EMULATOR`  | Path to an NES emulator executable (e.g., `mesen`, `fceux`) |
| `SNES_EMULATOR` | Path to an SNES emulator executable (e.g., `bsnes`, `snes9x`) |

## Setup

```bash
cargo build --release
```

1. Place your ROMs under `games/` (a sample `tetris.gb` is already there).
2. Export env vars if you want NES/SNES launching (for example `export NES_EMULATOR=mesen`).

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

NES/SNES titles inherit the key bindings from the external emulator you configure via the env vars.

## Folder Layout

```
games/
  tetris.gb         # sample Game Boy ROM
src/
  ...
```

Add more ROMs (GB/GBC/NES/SNES) to the `games/` folder and rerun the launcher whenever you want to
play.
