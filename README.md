# Retro Launcher (Game Boy / NES / SNES / ZX Spectrum / NDS / PlayStation / Nintendo 64 / GameCube)

This project is a lightweight retro game launcher with built-in Game Boy, NES, SNES, ZX Spectrum,
Nintendo DS, PlayStation, and Nintendo 64 emulator cores. Drop ROMs into the `games/` directory
and run one command to pick a title—no external emulators required for any of the handheld or
console cores listed above. GameCube titles still hand off to Dolphin automatically when it is
installed so you get full-speed emulation there as well.

## Requirements

- Rust 1.76+
- SDL2 development libraries (`libsdl2-dev`, `SDL2-devel`, or `brew install sdl2`)
- [Mupen64Plus](https://mupen64plus.org/) core + plugins (video, audio, input, and RSP). Install the `m64p`
  bundle on Windows/macOS or the `mupen64plus`/`mupen64plus-plugins` packages on Linux so the launcher can
  dynamically load `libmupen64plus` along with `mupen64plus-video-*`, `mupen64plus-audio-*`,
  `mupen64plus-input-*`, and `mupen64plus-rsp-*`.
- Dolphin emulator binary (optional, for full GameCube emulation—set `DOLPHIN_BIN` or place Dolphin on your `PATH`)
- Vulkan 1.1 runtime + GPU drivers (MoltenVK on macOS, `vulkan-loader`/`mesa-vulkan-drivers` on Linux, and up-to-date
  GPU drivers on Windows) for the built-in PlayStation core powered by `trapezoid-core`.
- A PlayStation BIOS (`SCPH1001.bin`, `SCPH5501.bin`, etc.) stored at `bios/ps1/` or pointed to with the
  `PS1_BIOS`/`PSX_BIOS` environment variable (or `--ps1-bios` on the CLI).
- (Optional) ZX Spectrum ROM dumps stored under `bios/zx/` (`48.rom` for Sinclair 48K or
  `128.rom.0`/`128.rom.1` for Sinclair 128K). Filenames are matched case-insensitively so you can keep
  your existing dumps. If you do not provide any, the launcher automatically falls back to the
  built-in ROMs shipped inside the ZX Spectrum core.
- Nothing else—every handheld plus Nintendo 64 ships in-tree or is loaded automatically at runtime, so you do
  not need BIOS dumps or ROM-specific patches for those systems.
## Setup

```bash
cargo build --release
```

1. Place your ROMs under `games/` (a sample `tetris.gb` is already there).
2. Just drop your ROMs (`.gb`, `.gbc`, `.nes`, `.sfc`, `.smc`, `.snes`, `.tap`, `.tzx`, `.sna`, `.nds`, `.cue`, `.exe`, `.n64`, `.z64`, `.v64`, `.iso`, `.gcm`, `.gcz`, `.gcn`, `.rvz`, `.ciso`) into `games/`.

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

PlayStation discs are identical—pass the `.cue` file (plus `--ps1-bios ...` if you have not set
`PS1_BIOS`/`PSX_BIOS`):

```bash
cargo run --release -- --rom "games/Crash Bandicoot.cue" --ps1-bios bios/ps1/scph1001.bin
```

> **Frame pacing**: Windows default to ~60 FPS so titles run at their intended speed. Pass
> `--limit-fps=false` only if you explicitly want uncapped rendering (useful for debugging).

### ZX Spectrum setup notes

ZX Spectrum ROM dumps can be dropped under `bios/zx/` (48K = `48.rom`, 128K = `128.rom.0` and
`128.rom.1`). Filenames are matched case-insensitively so existing dumps can simply be copied into
that directory. If both 48K and 128K dumps are present the 128K machine is preferred for AY sound
and memory paging. When no dumps are available the launcher automatically falls back to the
open-source ROMs embedded inside the ZX Spectrum core, so you can boot tapes out of the box.

Games themselves live in `games/` alongside every other system. The launcher accepts `.tap`,
standard-speed `.tzx`, and `.sna` snapshots for Spectrum titles. Tapes are automatically inserted
and start playing as soon as the emulator boots, including fast-loading support so you can reach
the game without manual `LOAD ""` loops. The launcher automatically drops into the loader state
and issues the `LOAD ""` command for you, so tapes begin streaming immediately (press `Insert` if
you need to restart the tape). Snapshots boot straight into the saved state. `.tzx` support
currently covers only standard-speed data blocks—if you bump into an unsupported block ID just
convert the tape to `.tap` (via `tzx2tap`/`czxtools`) and drop it back into `games/`.

### PlayStation setup notes

The PlayStation 1 core embeds [`trapezoid-core`](https://crates.io/crates/trapezoid-core) directly, so the
launcher needs a few extra assets on disk:

1. **BIOS** – place `SCPH1001.bin`, `SCPH5501.bin`, or another NTSC BIOS image under `bios/ps1/`
   (filenames are matched case-insensitively), or point the launcher at a BIOS by passing
   `--ps1-bios /path/to/SCPH1001.bin` or exporting `PS1_BIOS=/path/to/SCPH1001.bin`
   (`PSX_BIOS` is also honored). The file must exist and will be fed directly into the emulator core.
2. **ROM formats** – disc games should be launched via their `.cue` sheet so the emulator can follow
   the referenced `.bin` tracks; PS-EXE homebrew files (`.exe`) are also supported. Keep the `.bin`
   payload next to the `.cue` file just like you would for other PS1 frontends.
3. **Vulkan runtime** – ensure `vulkaninfo` works (MoltenVK on macOS, up-to-date AMD/NVIDIA/Intel
   drivers on Windows, and the `vulkan-loader`/`mesa-vulkan-drivers` packages on Linux).
4. **Memory cards** – the core automatically reads/writes `memcard0.mcd` and `memcard1.mcd` in the
   current working directory so saves persist between runs.

## Controller Support

Any SDL2-compatible gamepad works across every built-in core. The d-pad (or left stick) maps to the
console d-pads, `A/B/X/Y` map to the respective face buttons, and the shoulder buttons map to L/R. On
Nintendo 64 the right stick drives the C-buttons and the left trigger (or Back/Select) becomes the Z
trigger; PlayStation follows the same layout (A=Cross, B=Circle, X=Square, Y=Triangle) with analog
triggers feeding L2/R2, and GameCube mirrors that pattern for its C-stick and analog triggers. `Start` maps to START
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
| ZX Spectrum | Built-in Rust core (`rustzx-core`)                                                             |
| Nintendo DS | Built-in Rust core (`desmume-rs`)                                                             |
| PlayStation | Built-in [`trapezoid-core`](https://crates.io/crates/trapezoid-core`) + Vulkan renderer (via `vulkano`/`winit`) |
| Nintendo 64 | Embedded [Mupen64Plus](https://mupen64plus.org/) core loaded at runtime (libmupen64plus + plugins) |
| GameCube | **External** Dolphin binary (auto-detected via `DOLPHIN_BIN` or `PATH`); stub visualization runs only when Dolphin is missing |

All of the built-in rows (Game Boy through PlayStation and Nintendo 64) compile directly into the launcher binary.
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

### Controls (ZX Spectrum core)

- The PC keyboard maps 1:1 to the Sinclair keyboard matrix (`Shift`, letters, digits, space, and
  `Ctrl` → Symbol Shift). Arrow keys, `Caps Lock`, `Backspace`, and `End` emit the expected
  Sinclair compound keys (Caps Shift + digit combos).
- `Insert`: start/rewind the current tape, `Delete`: stop tape playback.
- `F3`/`F4`/`F5`: switch between 1×, 2×, and uncapped emulation speed; `F1`/`F2` quick-save and
  quick-load a pair of `.sna` snapshots in the working directory.
- Controllers: the d-pad or left stick drives a Kempston joystick, `A`/`B`/`X`/`Y` all trigger the
  fire button, `Start` sends ENTER, and `Select` sends SPACE to keep in-game menus reachable.

Kempston mouse input is not enabled in the launcher, so a regular mouse continues to work as the
system cursor instead of being captured.

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

### Controls (PlayStation core)

- Arrow keys: D-pad
- `Z`: Cross (X)
- `X`: Circle
- `A`: Square
- `S`: Triangle
- `Q`: L1, `W`: R1
- `1`: L2, `2`: R2
- `Enter`: START
- `Right Shift` (or `Space` / `Backspace`): SELECT
- `Esc` / window close: exit

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
bios/
  ps1/
    SCPH1001.bin    # place your BIOS dump here (or point --ps1-bios / PS1_BIOS to it)
src/
  ...
```

Add more ROMs (GB/GBC/NES/SNES/NDS/PS1/N64/GameCube) to the `games/` folder and rerun the launcher whenever you want to
play.
