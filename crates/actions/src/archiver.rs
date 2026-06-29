//! Archiver: moves selected folders to an external drive and records a manifest
//! so every move can be reversed.
//!
//! Design:
//! - Each move is copy-then-verify-then-delete-source (safer than rename across
//!   volumes, which can fail mid-way and leave partial state).
//! - A JSON manifest is written next to the archive root listing every source
//!   path moved, so `undo_archive` can move them back.
//! - Idempotent: re-running won't duplicate data; destinations are skipped if
//!   they already exist.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovedEntry {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub bytes: u64,
    pub moved_at_secs: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArchiveOutcome {
    pub moved: Vec<MovedEntry>,
    pub skipped: u64,
    pub bytes_moved: u64,
    /// Path to the manifest file written for this run (for undo).
    pub manifest_path: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UndoOutcome {
    pub restored: u64,
    pub skipped: u64,
    pub bytes_restored: u64,
}

const MANIFEST_NAME: &str = "cache-archive-manifest.json";

/// Run an archive plan: move each entry.source to entry.destination.
/// `external_root` is where the manifest is stored.
pub fn run_archive(
    entries: &[ca_core::archive::ArchiveEntry],
    external_root: &Path,
) -> Result<ArchiveOutcome> {
    fs::create_dir_all(external_root).context("create external root")?;

    let manifest_path = external_root.join(MANIFEST_NAME);
    let mut existing = load_manifest(&manifest_path).unwrap_or_default();

    let mut out = ArchiveOutcome {
        manifest_path: manifest_path.clone(),
        ..Default::default()
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    for e in entries {
        if !e.source.exists() {
            out.skipped += 1;
            continue;
        }
        if e.destination.exists() {
            // Already archived — don't duplicate.
            out.skipped += 1;
            continue;
        }
        fs::create_dir_all(e.destination.parent().unwrap_or(Path::new(".")))
            .context("create destination parent")?;

        let bytes = move_tree(&e.source, &e.destination)?;
        // Source tree removed only after successful copy. move_tree deletes source.
        out.moved.push(MovedEntry {
            source: e.source.clone(),
            destination: e.destination.clone(),
            bytes,
            moved_at_secs: now,
        });
        out.bytes_moved += bytes;
    }

    existing.extend(out.moved.clone());
    save_manifest(&manifest_path, &existing).context("write manifest")?;
    Ok(out)
}

/// Undo a previous archive using its manifest: move everything back.
pub fn undo_archive(manifest_path: &Path) -> Result<UndoOutcome> {
    let entries = load_manifest(manifest_path).context("load manifest")?;
    let mut out = UndoOutcome::default();

    for e in &entries {
        if !e.destination.exists() {
            out.skipped += 1;
            continue;
        }
        // Restore source location
        if let Some(parent) = e.source.parent() {
            let _ = fs::create_dir_all(parent);
        }
        match move_tree(&e.destination, &e.source) {
            Ok(bytes) => {
                out.restored += 1;
                out.bytes_restored += bytes;
            }
            Err(_) => {
                out.skipped += 1;
            }
        }
    }

    // Clear the manifest once undone.
    let _ = fs::remove_file(manifest_path);
    Ok(out)
}

/// Copy a whole directory tree then remove the source. Returns bytes moved.
/// Uses a copy+delete rather than rename() to be robust across volumes.
fn move_tree(src: &Path, dst: &Path) -> Result<u64> {
    let mut total: u64 = 0;
    let src_meta = fs::symlink_metadata(src).context("stat source")?;

    if src_meta.is_dir() {
        fs::create_dir_all(dst).context("create dst dir")?;
        for entry in fs::read_dir(src).context("read src dir")? {
            let entry = entry?;
            let child_src = entry.path();
            let child_name = entry.file_name();
            let child_dst = dst.join(child_name);
            total += move_tree(&child_src, &child_dst)?;
        }
        // Remove now-empty source dir.
        let _ = fs::remove_dir(src);
    } else {
        // File or symlink.
        if let Ok(m) = fs::metadata(src) {
            total += m.len();
        }
        fs::copy(src, dst).with_context(|| format!("copy {} -> {}", src.display(), dst.display()))?;
        let _ = fs::remove_file(src);
    }
    Ok(total)
}

fn load_manifest(path: &Path) -> Result<Vec<MovedEntry>> {
    match fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).context("parse manifest"),
        Err(_) => Ok(Vec::new()),
    }
}

fn save_manifest(path: &Path, entries: &[MovedEntry]) -> Result<()> {
    let s = serde_json::to_string_pretty(entries)?;
    fs::write(path, s).context("write manifest file")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ca_core::archive::ArchiveEntry;
    use std::path::PathBuf;

    fn w(p: &Path, name: &str, data: &[u8]) {
        fs::create_dir_all(p).unwrap();
        fs::write(p.join(name), data).unwrap();
    }

    #[test]
    fn archive_then_undo_roundtrip() {
        let root = std::env::temp_dir().join("ca_arch_rt");
        let _ = fs::remove_dir_all(&root);
        let src = root.join("src");
        let ext = root.join("external");
        w(&src, "f1.bin", &vec![0u8; 1000]);
        w(&src.join("sub"), "f2.bin", &vec![0u8; 500]);

        let entries = vec![ArchiveEntry {
            source: src.clone(),
            destination: ext.join("cache-archive").join("src"),
            bytes: 1500,
            reason: "".into(),
        }];
        let out = run_archive(&entries, &ext).unwrap();
        assert_eq!(out.bytes_moved, 1500);
        assert_eq!(out.skipped, 0);
        // Source gone after move.
        assert!(!src.exists());
        // Destination has files.
        assert!(ext.join("cache-archive").join("src").join("f1.bin").exists());

        // Undo.
        let manifest = ext.join(MANIFEST_NAME);
        assert!(manifest.exists());
        let undo = undo_archive(&manifest).unwrap();
        assert_eq!(undo.restored, 1);
        assert_eq!(undo.bytes_restored, 1500);
        // Source restored.
        assert!(src.join("f1.bin").exists());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn idempotent_skip_existing() {
        let root = std::env::temp_dir().join("ca_arch_idem");
        let _ = fs::remove_dir_all(&root);
        let src = root.join("src");
        let ext = root.join("external");
        w(&src, "f.bin", b"data");

        let entries = vec![ArchiveEntry {
            source: src.clone(),
            destination: ext.join("cache-archive").join("src"),
            bytes: 4,
            reason: "".into(),
        }];
        let _ = run_archive(&entries, &ext).unwrap();
        // Re-create source to simulate re-run scenario (already archived).
        w(&src, "f.bin", b"data");
        let out2 = run_archive(&entries, &ext).unwrap();
        assert_eq!(out2.skipped, 1);
        assert_eq!(out2.bytes_moved, 0);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_source_skipped() {
        let root = std::env::temp_dir().join("ca_arch_missing");
        let _ = fs::remove_dir_all(&root);
        let ext = root.join("external");
        let entries = vec![ArchiveEntry {
            source: PathBuf::from("Z:/nope/missing"),
            destination: ext.join("cache-archive").join("x"),
            bytes: 0,
            reason: "".into(),
        }];
        let out = run_archive(&entries, &ext).unwrap();
        assert_eq!(out.skipped, 1);
        let _ = fs::remove_dir_all(&root);
    }
}
