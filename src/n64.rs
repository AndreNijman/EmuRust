use std::env;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use dirs::{cache_dir, config_dir};
use flate2::read::GzDecoder;
use libloading::library_filename;
use log::{debug, info};
use mupen64plus::{Core, Plugin};
use tar::Archive;

pub fn run(rom_path: &Path, scale: u32, limit_fps: bool) -> Result<()> {
    let mut rom = fs::read(rom_path)
        .with_context(|| format!("failed to read Nintendo 64 ROM {}", rom_path.display()))?;
    let bundled = ensure_bundled_assets()?;
    let core_setup = load_core(bundled.as_ref())?;
    let plugin_dirs = plugin_search_dirs(core_setup.library_dir.as_deref(), bundled.as_ref());
    let plugins = resolve_plugins(&plugin_dirs)?;
    let config_dir = ensure_config_dir()?;
    let data_dir = resolve_data_dir(core_setup.library_dir.as_deref(), bundled.as_ref());

    if scale != 4 || !limit_fps {
        debug!("Nintendo 64 core uses the video plugin's window; --scale/--limit-fps are ignored");
    }

    if let Some(dir) = &config_dir {
        debug!("Using Mupen64Plus config directory at {}", dir.display());
    }
    if let Some(dir) = &data_dir {
        debug!(
            "Using Mupen64Plus shared data directory at {}",
            dir.display()
        );
    }

    info!(
        "Starting Nintendo 64 core via {}",
        core_setup.origin_description
    );

    let mut mupen = core_setup
        .core
        .start(config_dir.as_deref(), data_dir.as_deref())
        .map_err(|err| anyhow!(err))
        .context("failed to start Mupen64Plus core")?;

    mupen
        .open_rom(&mut rom)
        .map_err(|err| anyhow!(err))
        .with_context(|| format!("failed to open {}", rom_path.display()))?;

    for (kind, path) in plugins.ordered() {
        let plugin = Plugin::load_from_path(path)
            .with_context(|| format!("failed to load {kind} plugin at {}", path.display()))?;
        mupen
            .attach_plugin(plugin)
            .map_err(|err| anyhow!(err))
            .with_context(|| format!("failed to attach {kind} plugin {}", path.display()))?;
        debug!("Attached {kind} plugin from {}", path.display());
    }

    info!(
        "Nintendo 64 core running for {}; close the plugin window or press Esc inside it to exit",
        rom_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("ROM")
    );
    mupen
        .execute()
        .map_err(|err| anyhow!(err))
        .context("mupen64plus execution failed")?;
    Ok(())
}

struct CoreSetup {
    core: Core,
    library_dir: Option<PathBuf>,
    origin_description: String,
}

#[derive(Clone)]
struct BundledAssets {
    core_path: PathBuf,
    plugin_dir: PathBuf,
    data_dir: PathBuf,
    description: String,
}

fn load_core(bundled: Option<&BundledAssets>) -> Result<CoreSetup> {
    if let Some(bundle) = bundled {
        let core = Core::load_from_path(&bundle.core_path)
            .map_err(|err| anyhow!(err))
            .with_context(|| {
                format!(
                    "failed to load bundled core at {}",
                    bundle.core_path.display()
                )
            })?;
        return Ok(CoreSetup {
            library_dir: bundle.core_path.parent().map(|p| p.to_path_buf()),
            origin_description: format!("{} (auto-downloaded)", bundle.description),
            core,
        });
    }

    if let Some(explicit) = env_path("M64P_CORE_LIB") {
        let core = Core::load_from_path(&explicit)
            .map_err(|err| anyhow!(err))
            .with_context(|| format!("failed to load core library at {}", explicit.display()))?;
        let dir = explicit.parent().map(|p| p.to_path_buf());
        return Ok(CoreSetup {
            core,
            library_dir: dir,
            origin_description: format!("M64P_CORE_LIB ({})", explicit.display()),
        });
    }

    if let Some(root) = env_path("M64P_ROOT") {
        let filename = PathBuf::from(library_filename("mupen64plus"));
        for candidate in [
            root.join(&filename),
            root.join("lib").join(&filename),
            root.join("bin").join(&filename),
        ] {
            if candidate.exists() {
                let core = Core::load_from_path(&candidate)
                    .map_err(|err| anyhow!(err))
                    .with_context(|| {
                        format!("failed to load core library from {}", candidate.display())
                    })?;
                return Ok(CoreSetup {
                    library_dir: candidate.parent().map(|p| p.to_path_buf()),
                    origin_description: format!("M64P_ROOT ({})", candidate.display()),
                    core,
                });
            }
        }
    }

    let core = Core::load_from_system()
        .map_err(|err| anyhow!(err))
        .context("failed to locate libmupen64plus via system search paths")?;
    Ok(CoreSetup {
        core,
        library_dir: None,
        origin_description: "system library paths".into(),
    })
}

struct PluginSet {
    video: PathBuf,
    audio: PathBuf,
    input: PathBuf,
    rsp: PathBuf,
}

impl PluginSet {
    fn ordered(&self) -> [(&'static str, &PathBuf); 4] {
        [
            ("video", &self.video),
            ("audio", &self.audio),
            ("input", &self.input),
            ("RSP", &self.rsp),
        ]
    }
}

fn resolve_plugins(search_dirs: &[PathBuf]) -> Result<PluginSet> {
    let video = find_plugin("video", "M64P_VIDEO", VIDEO_PLUGIN_CANDIDATES, search_dirs)?;
    let audio = find_plugin("audio", "M64P_AUDIO", AUDIO_PLUGIN_CANDIDATES, search_dirs)?;
    let input = find_plugin("input", "M64P_INPUT", INPUT_PLUGIN_CANDIDATES, search_dirs)?;
    let rsp = find_plugin("RSP", "M64P_RSP", RSP_PLUGIN_CANDIDATES, search_dirs)?;
    Ok(PluginSet {
        video,
        audio,
        input,
        rsp,
    })
}

fn ensure_config_dir() -> Result<Option<PathBuf>> {
    if let Some(explicit) = env_path("M64P_CONFIG_DIR") {
        fs::create_dir_all(&explicit).with_context(|| {
            format!(
                "failed to create configured Mupen64Plus directory at {}",
                explicit.display()
            )
        })?;
        return Ok(Some(explicit));
    }

    match config_dir() {
        Some(base) => {
            let path = base.join("retro-launcher").join("mupen64plus");
            fs::create_dir_all(&path)
                .with_context(|| format!("failed to create {}", path.display()))?;
            Ok(Some(path))
        }
        None => Ok(None),
    }
}

fn resolve_data_dir(core_dir: Option<&Path>, bundled: Option<&BundledAssets>) -> Option<PathBuf> {
    if let Some(explicit) = env_path("M64P_DATA_DIR") {
        if explicit.exists() {
            return Some(explicit);
        }
    }
    if let Some(bundle) = bundled {
        return Some(bundle.data_dir.clone());
    }
    let mut candidates = Vec::new();
    if let Some(root) = env_path("M64P_ROOT") {
        candidates.push(root.join("share").join("mupen64plus"));
        candidates.push(root.join("data"));
    }
    if let Some(dir) = core_dir {
        candidates.push(dir.to_path_buf());
        if let Some(parent) = dir.parent() {
            candidates.push(parent.join("share").join("mupen64plus"));
        }
    }
    candidates.extend(default_data_dirs());
    candidates.into_iter().find(|path| path.exists())
}

fn plugin_search_dirs(core_dir: Option<&Path>, bundled: Option<&BundledAssets>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(bundle) = bundled {
        push_unique(&mut dirs, bundle.plugin_dir.clone());
    }
    dirs.extend(env_paths("M64P_PLUGIN_DIR"));
    if let Some(root) = env_path("M64P_ROOT") {
        push_unique(&mut dirs, root.clone());
        push_unique(&mut dirs, root.join("plugins"));
        push_unique(&mut dirs, root.join("bin"));
        push_unique(&mut dirs, root.join("lib"));
    }
    if let Some(dir) = core_dir {
        push_unique(&mut dirs, dir.to_path_buf());
        if let Some(parent) = dir.parent() {
            push_unique(&mut dirs, parent.join("mupen64plus"));
        }
    }
    dirs.extend(default_plugin_dirs());
    dirs
}

fn find_plugin(
    kind: &str,
    env_var: &str,
    candidates: &[&str],
    search_dirs: &[PathBuf],
) -> Result<PathBuf> {
    if let Some(explicit) = env_path(env_var) {
        if explicit.exists() {
            return Ok(explicit);
        } else {
            bail!(
                "{} plugin override {}={} does not exist",
                kind,
                env_var,
                explicit.display()
            );
        }
    }

    let mut attempts = Vec::new();
    for dir in search_dirs {
        for candidate in candidates {
            for file_name in candidate_filenames(candidate) {
                let path = dir.join(&file_name);
                attempts.push(path.clone());
                if path.exists() {
                    return Ok(path);
                }
            }
        }
    }

    let attempted = attempts
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    bail!(
        "unable to locate {kind} plugin; set {env_var} or install one of {:?} (looked in: {attempted})",
        candidates
    );
}

fn candidate_filenames(candidate: &str) -> Vec<PathBuf> {
    let mut names = Vec::new();
    names.push(PathBuf::from(library_filename(candidate)));
    let plain = PathBuf::from(candidate);
    if !names.contains(&plain) {
        names.push(plain.clone());
    }
    if plain.extension().is_none() {
        #[cfg(target_os = "linux")]
        {
            let so = PathBuf::from(format!("{candidate}.so"));
            if !names.contains(&so) {
                names.push(so);
            }
        }
        #[cfg(target_os = "macos")]
        {
            let dylib = PathBuf::from(format!("{candidate}.dylib"));
            if !names.contains(&dylib) {
                names.push(dylib);
            }
        }
        #[cfg(target_os = "windows")]
        {
            let dll = PathBuf::from(format!("{candidate}.dll"));
            if !names.contains(&dll) {
                names.push(dll);
            }
        }
    }
    names
}

fn env_path(var: &str) -> Option<PathBuf> {
    env::var_os(var)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn env_paths(var: &str) -> Vec<PathBuf> {
    env::var_os(var)
        .map(|value| env::split_paths(&value).collect())
        .unwrap_or_default()
}

fn push_unique(vec: &mut Vec<PathBuf>, path: PathBuf) {
    if vec.iter().any(|entry| entry == &path) {
        return;
    }
    vec.push(path);
}

fn default_plugin_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(target_os = "linux")]
    {
        dirs.push(PathBuf::from("/usr/lib/mupen64plus"));
        dirs.push(PathBuf::from("/usr/local/lib/mupen64plus"));
        dirs.push(PathBuf::from("/usr/lib64/mupen64plus"));
    }
    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/usr/local/lib/mupen64plus"));
        dirs.push(PathBuf::from("/opt/homebrew/lib/mupen64plus"));
    }
    #[cfg(target_os = "windows")]
    {
        dirs.push(PathBuf::from(r"C:\Program Files\m64p"));
        dirs.push(PathBuf::from(r"C:\Program Files (x86)\m64p"));
    }
    dirs
}

fn default_data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(target_os = "linux")]
    {
        dirs.push(PathBuf::from("/usr/share/mupen64plus"));
        dirs.push(PathBuf::from("/usr/local/share/mupen64plus"));
    }
    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/usr/local/share/mupen64plus"));
        dirs.push(PathBuf::from("/opt/homebrew/share/mupen64plus"));
    }
    #[cfg(target_os = "windows")]
    {
        dirs.push(PathBuf::from(r"C:\Program Files\m64p"));
        dirs.push(PathBuf::from(r"C:\Program Files (x86)\m64p"));
    }
    dirs
}

const VIDEO_PLUGIN_CANDIDATES: &[&str] = &[
    "mupen64plus-video-angrylion-plus",
    "mupen64plus-video-glide64mk2",
    "mupen64plus-video-rice",
    "mupen64plus-video-paraLLEl",
];

const AUDIO_PLUGIN_CANDIDATES: &[&str] = &[
    "mupen64plus-audio-sdl",
    "mupen64plus-audio-sdl2",
    "mupen64plus-audio-omx",
];

const INPUT_PLUGIN_CANDIDATES: &[&str] = &[
    "mupen64plus-input-sdl",
    "mupen64plus-input-raphnetraw",
    "mupen64plus-input-qt",
];

const RSP_PLUGIN_CANDIDATES: &[&str] = &[
    "mupen64plus-rsp-hle",
    "mupen64plus-rsp-cxd4-sse2",
    "mupen64plus-rsp-z64",
];

fn ensure_bundled_assets() -> Result<Option<BundledAssets>> {
    #[cfg(target_os = "linux")]
    {
        const VERSION: &str = "2.5.9";
        const DIR_NAME: &str = "mupen64plus-bundle-linux64-2.5.9";
        const URL: &str = "https://github.com/Mupen64Plus/mupen64plus-core/releases/download/2.5.9/mupen64plus-bundle-linux64-2.5.9.tar.gz";

        let cache_root = cache_dir()
            .unwrap_or_else(|| PathBuf::from("target/cache"))
            .join("retro-launcher")
            .join("mupen64plus")
            .join("linux64")
            .join(VERSION);
        let extracted = cache_root.join(DIR_NAME);
        let core_path = extracted.join("libmupen64plus.so");
        if !core_path.exists() {
            info!(
                "Downloading bundled Mupen64Plus {} (once per machine)...",
                VERSION
            );
            download_and_extract(URL, &cache_root)
                .context("failed to bootstrap bundled Mupen64Plus")?;
        }
        if core_path.exists() {
            return Ok(Some(BundledAssets {
                core_path,
                plugin_dir: extracted.clone(),
                data_dir: extracted,
                description: format!("Mupen64Plus bundle {}", VERSION),
            }));
        }
        Ok(None)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(None)
    }
}

fn download_and_extract(url: &str, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    let response = ureq::get(url)
        .call()
        .map_err(|err| anyhow!("failed to download {}: {}", url, err))?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut bytes)
        .context("failed to read downloaded bundle")?;
    let cursor = Cursor::new(bytes);
    let decoder = GzDecoder::new(cursor);
    let mut archive = Archive::new(decoder);
    archive
        .unpack(dest)
        .context("failed to extract Mupen64Plus bundle")?;
    Ok(())
}
