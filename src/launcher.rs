use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use log::warn;
use crate::systems::{detect_system, GameSystem};

const SUPPORTED_EXTENSIONS: [&str; 14] = [
    "gb", "gbc", "nes", "sfc", "smc", "snes", "nds", "iso", "gcm", "gcz", "gcn", "ciso", "dol",
    "rvz",
];

pub fn select_game(dir: &Path) -> Result<PathBuf> {
    let mut games_by_system: BTreeMap<GameSystem, Vec<PathBuf>> = BTreeMap::new();

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
                Ok(system) => games_by_system.entry(system).or_default().push(path),
                Err(err) => warn!("Skipping {}: {}", path.display(), err),
            };
        });

    if games_by_system.is_empty() {
        bail!("no compatible ROMs found under {}", dir.display());
    }

    let mut systems: Vec<(GameSystem, Vec<PathBuf>)> = games_by_system
        .into_iter()
        .map(|(system, mut games)| {
            games.sort();
            (system, games)
        })
        .collect();
    systems.sort_by_key(|(system, _)| system.label());

    loop {
        println!("\n=== Game Launcher ===");
        for (idx, (system, games)) in systems.iter().enumerate() {
            println!(
                "{:>2}. {} ({} game{})",
                idx + 1,
                system,
                games.len(),
                if games.len() == 1 { "" } else { "s" }
            );
        }

        let console_choice =
            prompt_number("Select a console by number: ", 1, systems.len()) - 1;

        let (system, games) = &systems[console_choice];
        println!("\n-- {} --", system);
        for (idx, entry) in games.iter().enumerate() {
            let name = entry
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("<unknown>");
            println!("{:>2}. {}", idx + 1, name);
        }
        println!(" 0. Back to console list");

        let game_choice =
            prompt_number("Select a game (0 to go back): ", 0, games.len());
        if game_choice == 0 {
            continue;
        }

        return Ok(games[game_choice - 1].clone());
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
