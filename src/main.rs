use clap::{Parser, Subcommand};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Error, ErrorKind, Read, Result};
use std::path::{Path, PathBuf};
use std::process;

#[derive(Parser)]
#[command(version, about = "File Integrity Monitor")]
struct Cli {
    #[command(subcommand)]
    mode: Mode,
}

#[derive(Subcommand)]
enum Mode {
    /// Scan a folder and save a baseline
    Init {
        /// Folder to scan
        folder: PathBuf,
        /// Overwrite an existing baseline or save despite skipped items
        #[arg(long)]
        force: bool,
    },
    /// Compare a folder against a saved baseline
    Check {
        /// Folder to scan
        folder: PathBuf,
    },
}

#[derive(Debug, PartialEq)]
enum Change {
    Added(PathBuf),
    Modified(PathBuf),
    Deleted(PathBuf),
}

fn hash_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    let hex: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();

    Ok(hex)
}

fn scan_dir(
    dir: &Path,
    results: &mut BTreeMap<PathBuf, String>,
    skipped: &mut Vec<(PathBuf, String)>,
) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            // eprintln!("Skipping folder {}: {}", dir.display(), e);
            skipped.push((dir.to_path_buf(), e.to_string()));
            return;
        }
    };

    for entry in entries {
        let path = match entry {
            Ok(entry) => entry.path(),
            Err(e) => {
                // eprintln!("Skipping entry in {}: {}", dir.display(), e);
                skipped.push((dir.to_path_buf(), e.to_string()));
                continue;
            }
        };

        if path.is_dir() {
            scan_dir(&path, results, skipped);
        } else if path.is_file() {
            match hash_file(&path) {
                Ok(hash) => {
                    results.insert(path, hash);
                }
                Err(e) => skipped.push((path, e.to_string())),
            }
        }
    }
}

fn save_baseline(folder: &Path, results: &BTreeMap<PathBuf, String>, file: &str) -> Result<()> {
    let mut text = format!("# folder: {}\n", folder.display());
    for (path, hash) in results {
        text.push_str(&format!("{}  {}\n", hash, path.display()));
    }
    fs::write(file, text)
}

fn load_baseline(file: &str) -> Result<(String, BTreeMap<PathBuf, String>)> {
    let text = fs::read_to_string(file)?;
    let mut lines = text.lines();

    let first = lines.next().unwrap_or("");
    let folder = match first.strip_prefix("# folder: ") {
        Some(f) => f.to_string(),
        None => {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "baseline header missing or malformed",
            ));
        }
    };

    let mut map = BTreeMap::new();
    for (i, line) in lines.enumerate() {
        match line.split_once("  ") {
            Some((hash, path)) => {
                map.insert(PathBuf::from(path), hash.to_string());
            }
            None => {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    format!("baseline line {} is malformed", i + 2),
                ));
            }
        }
    }
    Ok((folder, map))
}

fn compare(old: &BTreeMap<PathBuf, String>, new: &BTreeMap<PathBuf, String>) -> Vec<Change> {
    let mut changes = Vec::new();

    for (path, new_hash) in new {
        match old.get(path) {
            None => changes.push(Change::Added(path.clone())),
            Some(old_hash) if old_hash != new_hash => changes.push(Change::Modified(path.clone())),
            Some(_) => {}
        }
    }

    for path in old.keys() {
        if !new.contains_key(path) {
            changes.push(Change::Deleted(path.clone()));
        }
    }

    changes
}

fn run() {
    let cli = Cli::parse();
    let folder_arg = match &cli.mode {
        Mode::Init { folder, .. } => folder,
        Mode::Check { folder } => folder,
    };
    let folder = match fs::canonicalize(folder_arg) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Cannot use '{}': {}", folder_arg.display(), e);
            process::exit(2);
        }
    };
    if !folder.is_dir() {
        eprintln!("'{}' is not a folder", folder.display());
        process::exit(2);
    }

    let mut results = BTreeMap::new();
    let mut skipped = Vec::new();
    scan_dir(&folder, &mut results, &mut skipped);

    if !skipped.is_empty() {
        println!("WARNING: {} items skipped:", skipped.len());
        for (path, reason) in &skipped {
            println!("* {} -> {}", path.display(), reason);
        }
    }

    match cli.mode {
        Mode::Init { force, .. } => {
            if !skipped.is_empty() && !force {
                eprintln!("Refusing to save: items were skipped. Use --force to override.");
                process::exit(2);
            }
            if Path::new("baseline.txt").exists() && !force {
                eprintln!("baseline.txt already exists. Use --force to overwrite.");
                process::exit(2);
            }
            match save_baseline(&folder, &results, "baseline.txt") {
                Ok(()) => println!("Baseline saved: {} files", results.len()),
                Err(e) => {
                    eprintln!("Could not save baseline: {}", e);
                    process::exit(2);
                }
            }
        }
        Mode::Check { .. } => {
            let (base_folder, old) = match load_baseline("baseline.txt") {
                Ok(loaded) => loaded,
                Err(e) => {
                    eprintln!("Could not load baseline: {}", e);
                    process::exit(2);
                }
            };
            if base_folder != folder.display().to_string() {
                eprintln!(
                    "Baseline was made from '{}', not '{}'. Refusing to compare.",
                    base_folder,
                    folder.display()
                );
                process::exit(2);
            }

            let changes = compare(&old, &results);
            for change in &changes {
                match change {
                    Change::Added(p) => println!("ADDED {}", p.display()),
                    Change::Modified(p) => println!("MODIFIED {}", p.display()),
                    Change::Deleted(p) => println!("DELETED {}", p.display()),
                }
            }

            if changes.is_empty() && skipped.is_empty() {
                println!("No changes");
            } else {
                process::exit(1);
            }
        }
    }
}

fn main() {
    run();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(items: &[(&str, &str)]) -> BTreeMap<PathBuf, String> {
        let mut m = BTreeMap::new();
        for (p, h) in items {
            m.insert(PathBuf::from(p), h.to_string());
        }
        m
    }

    #[test]
    fn detects_added_modified_deleted() {
        let old = map(&[("a", "1"), ("b", "2"), ("c", "3")]);
        let new = map(&[("a", "1"), ("b", "x"), ("d", "4")]);

        assert_eq!(
            compare(&old, &new),
            vec![
                Change::Modified(PathBuf::from("b")),
                Change::Added(PathBuf::from("d")),
                Change::Deleted(PathBuf::from("c"))
            ]
        );
    }

    #[test]
    fn detect_added_deleted_for_rename() {
        let old = map(&[("a", "1"), ("b", "2"), ("c", "3")]);
        let new = map(&[("a", "1"), ("e", "2"), ("c", "3")]);

        assert_eq!(
            compare(&old, &new),
            vec![
                Change::Added(PathBuf::from("e")),
                Change::Deleted(PathBuf::from("b"))
            ]
        );
    }

    #[test]
    fn identical_maps_have_no_changes() {
        let old = map(&[("a", "1")]);
        assert!(compare(&old, &old).is_empty());
    }
}
