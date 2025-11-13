# Retro Launcher (Game Boy / NES / SNES / NDS / Nintendo 64 / GameCube)

This project is a lightweight retro game launcher with built-in Game Boy, NES, SNES, Nintendo DS, and
Nintendo 64 emulator cores (all rendered with SDL2). Drop ROMs into the `games/` directory and run
one command to pick a title—no external emulators required for any of the handheld or console cores
listed above. GameCube titles still hand off to Dolphin automatically when it is installed so you get
full-speed emulation there as well.

## Requirements

- Rust 1.76+
- SDL2 development libraries (`libsdl2-dev`, `SDL2-devel`, or `brew install sdl2`)
- [Mupen64Plus](https://mupen64plus.org/) core + plugins (video, audio, input, and RSP). Install the `m64p`
  bundle on Windows/macOS or the `mupen64plus`/`mupen64plus-plugins` packages on Linux so the launcher can
  dynamically load `libmupen64plus` along with `mupen64plus-video-*`, `mupen64plus-audio-*`,
  `mupen64plus-input-*`, and `mupen64plus-rsp-*`.
- Dolphin emulator binary (optional, for full GameCube emulation—set `DOLPHIN_BIN` or place Dolphin on your `PATH`)
- Nothing else—every handheld plus Nintendo 64 ships in-tree or is loaded automatically at runtime, so you do
  not need BIOS dumps or ROM-specific patches for those systems.
## Setup

```bash
cargo build --release
```

1. Place your ROMs under `games/` (a sample `tetris.gb` is already there).
2. Just drop your ROMs (`.gb`, `.gbc`, `.nes`, `.sfc`, `.smc`, `.snes`, `.nds`, `.n64`, `.z64`, `.v64`, `.iso`, `.gcm`, `.gcz`, `.gcn`, `.rvz`, `.ciso`) into `games/`.

### Nintendo 64 setup notes

The N64 core embeds [Mupen64Plus](https://m64p.github.io/) directly, so the launcher simply loads the shared
libraries you already have installed. It scans the locations below automatically:

- `M64P_ROOT` (if set) plus its `lib`, `bin`, and `plugins` subdirectories
- Explicit overrides: `M64P_CORE_LIB`, `M64P_PLUGIN_DIR`, `M64P_VIDEO`, `M64P_AUDIO`, `M64P_INPUT`, `M64P_RSP`,
  and `M64P_DATA_DIR`
- Standard system locations such as `/usr/lib/mupen64plus`, `/usr/local/lib/mupen64plus`,
  `/usr/share/mupen64plus`, `/opt/homebrew/Cellar/mupen64plus/*`, and the default `m64p` installation folders
  on Windows (`C:\Program Files\m64p` etc.)
- **Linux bonus**: if none of the locations above contain the core, the launcher automatically downloads the
  official `mupen64plus-bundle-linux64-2.5.9` release from GitHub, extracts it under
  `${XDG_CACHE_HOME:-~/.cache}/retro-launcher/mupen64plus/`, and loads the core/plugins from there. That means
  a clean Linux install can boot Nintendo 64 titles with zero manual setup or environment variables.

If the launcher cannot find one of the required shared libraries it prints the exact file names it tried to
find and exits gracefully. The first launch also creates a config directory (e.g.
`$XDG_CONFIG_HOME/retro-launcher/mupen64plus`) where Mupen64Plus keeps its own `mupen64plus.cfg`. Feel free to
edit that file directly if you want to remap controller bindings or advanced video settings—the launcher will
re-use it on subsequent boots.

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

Nintendo 64 titles work the same way—just point at one of the supported cartridge images:

```bash
cargo run --release -- --rom "games/Mario Kart 64.z64"
```

> **Frame pacing**: Windows default to ~60 FPS so titles run at their intended speed. Pass
> `--limit-fps=false` only if you explicitly want uncapped rendering (useful for debugging).

## Controller Support

Any SDL2-compatible gamepad works across every built-in core. The d-pad (or left stick) maps to the
console d-pads, `A/B/X/Y` map to the respective face buttons, and the shoulder buttons map to L/R. On
Nintendo 64 the right stick drives the C-buttons and the left trigger (or Back/Select) becomes the Z
trigger; GameCube follows the same pattern for its C-stick and analog triggers. `Start` maps to START
everywhere, while `Back/Select` continues to feed SELECT on systems that have it. Nintendo DS titles
still require a mouse for the touchscreen, but all handheld/console buttons can be driven from the
controller.

Keyboard bindings are always active alongside controllers, so you can mix analog sticks with the key
layout below without toggling any settings.

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
| Nintendo 64 | Embedded [Mupen64Plus](https://mupen64plus.org/) core loaded at runtime (libmupen64plus + plugins) |
| GameCube | **External** Dolphin binary (auto-detected via `DOLPHIN_BIN` or `PATH`); stub visualization runs only when Dolphin is missing |

All of the built-in rows (Game Boy through Nintendo 64) compile directly into the launcher binary.
GameCube requires the external Dolphin install described above in order to play games; without it
you’ll just see the metadata overlay and placeholder graphics.

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

### Controls (Nintendo 64 core)

- Arrow keys / Controller d-pad or left stick: analog stick (full range) plus the digital pad
- `X`: A button
- `Z`: B button
- `I` / `K` / `J` / `L` or controller right stick: C-Up / C-Down / C-Left / C-Right
- `Q`: L button
- `W`: R button
- `E` or either Shift key / controller Back-Select / left trigger: Z trigger
- `Enter`: START
- `Esc` / window close: exit
  > All controller + keyboard input is routed through `mupen64plus-input-sdl`, so any SDL-compatible pad is
  > auto-detected alongside the keyboard. Edit `mupen64plus.cfg` under the launcher’s config directory if you
  > want to customize bindings or sensitivity—the defaults mirror the layout above.

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

Add more ROMs (GB/GBC/NES/SNES/NDS/N64/GameCube) to the `games/` folder and rerun the launcher whenever you want to
play.
