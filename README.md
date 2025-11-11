# Game Boy Emulator (Rust)

This emulator is built in Rust on top of the
[`gameboy_core`](https://crates.io/crates/gameboy_core) library. The heavy lifting (CPU/PPU/APU,
MBCs, timers) comes from the upstream crate, while this repository provides a fast SDL2 renderer,
keyboard controls, and a flexible CLI for running ROMs headlessly or interactively. The interactive
mode renders the DMG LCD in real time, so you get full graphical output whenever `--interactive` is
used.

## Features

- Accurate emulation pipeline from `gameboy_core` (DMG + most common MBCs, timers, interrupts)
- SDL2-based interactive window with 4-tone DMG palette, scaling, vsync, and keyboard controls
- Real-time audio playback streamed through SDL2 (stereo f32 output)
- Headless CLI options for deterministic frame or cycle counts (useful for automation/tests)
- Friendly command-line interface (`clap`) and logging (`env_logger`)
- Memory introspection hooks (one-shot dumps + per-frame watch streams for automation/bots)
- Simple code layout for future extensions (savestates, configurable input, web builds, etc.)

## Requirements

| Tool | Notes |
|------|-------|
| Rust | Latest stable (1.76+) recommended. Install via [rustup](https://rustup.rs). |
| SDL2 development libraries | Needed by the bundled SDL2 bindings. <br>• Debian/Ubuntu: `sudo apt install libsdl2-dev` <br>• Fedora: `sudo dnf install SDL2-devel` <br>• macOS (Homebrew): `brew install sdl2` <br>• Windows: install [SDL2 development libs](https://github.com/libsdl-org/SDL/releases) and ensure they are discoverable via pkg-config. |

## Installation

```bash
git clone https://github.com/andre/gameboy.git
cd gameboy
cargo build --release      # first build downloads dependencies
```

The compiled binary lives at `target/release/gameboy`. During development you can also run directly
with `cargo run`.

## Usage

### Interactive mode (window + keyboard)

```bash
cargo run --release -- ./tetris.gb --interactive --limit-fps --scale 4
```

- `--interactive` opens the SDL2 window and streams frames in real time.
- `--limit-fps` enables vsync (~60 FPS). Omit it to run as fast as possible.
- `--scale` resizes the window (`1`, `2`, `4`, `8`, …). Use `--scale 1` for native 160×144 output.
- `--ai` enables the experimental AI controller pipeline (currently a stub). When active, the emulator
  begins capturing Game Boy Tetris RAM (playfield, level, lines, next piece) each frame and logging
  summary stats so future learning agents can act on the state.
- Audio is streamed automatically through SDL2, so if your system audio is configured you should
  hear the ROM immediately.

**Controls**

| Action | Keys |
|--------|------|
| D-Pad  | Arrow keys |
| A      | `Z` |
| B      | `X` |
| START  | `Enter` |
| SELECT | `Shift`, `Space`, or `Backspace` |
| Quit   | `Esc` or close the window |

### Headless mode (batch/automation)

```bash
# Run a finite number of frames (headless)
cargo run -- ./tetris.gb --frames 120

# Execute an exact number of CPU cycles (useful for profiling)
cargo run -- ./tetris.gb --cycles 1_000_000

# Run forever without a window (Ctrl+C to stop)
cargo run -- ./tetris.gb --frames 0
```

Audio output remains enabled in headless mode, so you'll still hear the ROM unless you mute the
system output.

### Memory dumps and automation feeds

Tetris automation (or any ROM analysis) typically needs the raw RAM/VRAM contents. Two helpers are
available:

1. **One-shot dumps** – capture any address range after the run completes:

    ```bash
    # Dump 0x200 bytes starting at 0xC000 as a hex table
    cargo run -- ./tetris.gb --frames 120 --dump-range 0xC000:0x200

    # Same dump, but write raw binary bytes to a file
    cargo run -- ./tetris.gb --frames 120 \
      --dump-range 0xC000:0x200 \
      --dump-output wram.bin \
      --dump-format binary
    ```

2. **Live watch stream** – emit JSON lines after every VBlank with the bytes you care about. This
   is ideal for connecting external automation/bot processes:

    ```bash
    # Stream Game Boy Tetris playfield (10x20 bytes at 0xC0A0) + next-piece state
    cargo run --release -- ./tetris.gb --interactive \
      --watch board:0xC0A0:200 \
      --watch state:0xC200:32 \
      --watch-output tetris_watches.jsonl
    ```

    Each line looks like:

    ```json
    {"frame":128,"watches":[{"name":"board","start":49248,"data_hex":"000000..."}]}
    ```

    Use hex addresses or decimal; lengths accept the same. For Game Boy Tetris the playfield lives
    at `0xC0A0` (10×20 board) and various gameplay flags sit near `0xC200`, but you can point watches
    at any RAM location you need.

### CLI reference

| Flag | Default | Description |
|------|---------|-------------|
| `rom` | *(required)* | Path to `.gb` / `.gbc` file. |
| `--frames <n>` | `60` | Frames to execute; `0` = infinite. Ignored if `--interactive` is set. |
| `--cycles <n>` | *none* | Execute precise CPU cycles instead of frames. |
| `--interactive` | off | Launch SDL window and enable keyboard controls. |
| `--scale <n>` | `4` | Window scale factor in interactive mode. |
| `--limit-fps` | off | Enable vsync/60 FPS cap in interactive mode. |

## Development

- Format: `cargo fmt`
- Lint: `cargo clippy --all-targets`
- Tests: `cargo test`

All commands run from the repository root.

## Roadmap / Ideas

- Configurable key bindings and controller/gamepad support
- Savestates & SRAM persistence helpers
- WebAssembly/WebGPU frontend built on the same core
