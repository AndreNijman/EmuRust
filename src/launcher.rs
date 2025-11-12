use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

const SUPPORTED_EXTENSIONS: [&str; 7] = ["gb", "gbc", "nes", "sfc", "smc", "snes", "nds"];

pub fn select_game(dir: &Path) -> Result<PathBuf> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("failed to read games directory at {}", dir.display()))?
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.is_file() {
                let ext = path.extension()?.to_str()?.to_ascii_lowercase();
                if SUPPORTED_EXTENSIONS.contains(&ext.as_str()) {
                    return Some(path);
                }
            }
            None
        })
        .collect();

    entries.sort();

    if entries.is_empty() {
        bail!("no compatible ROMs found under {}", dir.display());
    }

    println!("=== Game Launcher ===");
    for (idx, entry) in entries.iter().enumerate() {
        let name = entry
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("<unknown>");
        println!("{:>2}. {}", idx + 1, name);
    }

    loop {
        print!("Select a game by number: ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin().read_line(&mut input).ok();
        if let Ok(choice) = input.trim().parse::<usize>() {
            if choice >= 1 && choice <= entries.len() {
                return Ok(entries[choice - 1].clone());
            }
        }
        println!(
            "Invalid selection. Please enter a number between 1 and {}.",
            entries.len()
        );
    }
}
