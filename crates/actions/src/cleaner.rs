//! Cache cleaner. Deletes the *contents* of a folder (keeps the folder itself),
//! mirroring the old Python behavior but with byte-accurate accounting.
//!
//! Files that are in use or access-denied are counted as skipped, not errors.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CleanOutcome {
    /// Bytes actually freed (verified from file metadata before deletion).
    pub freed_bytes: u64,
    /// Number of files deleted.
    pub files_removed: u64,
    /// Number of folders removed.
    pub folders_removed: u64,
    /// Files/folders we could not delete (locked, denied).
    pub skipped: u64,
}

impl CleanOutcome {
    pub fn total(&self) -> u64 {
        self.files_removed + self.folders_removed + self.skipped
    }
}

#[derive(Debug, Error)]
pub enum CleanError {
    #[error("path does not exist: {0}")]
    NotFound(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Delete everything inside `path`, but keep `path` itself.
pub fn clean_folder(path: &Path) -> Result<CleanOutcome, CleanError> {
    if !path.exists() {
        return Err(CleanError::NotFound(path.display().to_string()));
    }

    let mut out = CleanOutcome::default();
    let entries = match fs::read_dir(path) {
        Ok(e) => e,
        Err(e) => {
            // Can't even read the dir: treat as one skip, surface io error.
            return Err(CleanError::Io(e));
        }
    };

    for entry in entries.flatten() {
        let p = entry.path();
        let meta = match fs::symlink_metadata(&p) {
            Ok(m) => m,
            Err(_) => {
                out.skipped += 1;
                continue;
            }
        };

        let is_dir = meta.is_dir();
        let freed = if is_dir { dir_tree_size(&p) } else { meta.len() };
        let removed = if is_dir { remove_dir(&p) } else { remove_file(&p) };

        if removed {
            if is_dir {
                out.folders_removed += 1;
            } else {
                out.files_removed += 1;
            }
            out.freed_bytes += freed;
        } else {
            out.skipped += 1;
        }
    }

    Ok(out)
}

/// Delete a single file. Returns the bytes freed.
pub fn clean_file(path: &Path) -> Result<u64, CleanError> {
    if !path.exists() {
        return Err(CleanError::NotFound(path.display().to_string()));
    }
    let meta = fs::symlink_metadata(path)?;
    if meta.is_dir() {
        return Err(CleanError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "cannot delete directory as a single file",
        )));
    }
    let len = meta.len();
    fs::remove_file(path)?;
    Ok(len)
}

/// Sum every file under a directory recursively.
fn dir_tree_size(dir: &Path) -> u64 {
    let mut total: u64 = 0;
    for res in walkdir::WalkDir::new(dir).into_iter() {
        let Ok(e) = res else { continue };
        if let Ok(m) = e.metadata() {
            if m.is_file() {
                total += m.len();
            }
        }
    }
    total
}

fn remove_file(p: &Path) -> bool {
    match fs::remove_file(p) {
        Ok(_) => true,
        Err(_) => match fs::remove_dir(p) {
            // Some entries report as file but are actually empty dirs/symlinks.
            Ok(_) => true,
            Err(_) => false,
        },
    }
}

fn remove_dir(p: &Path) -> bool {
    match fs::remove_dir_all(p) {
        Ok(_) => true,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("ca_clean_{}", name));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn freed_bytes_accurate_not_item_count() {
        // Regression: must report BYTES freed, not number of items.
        let dir = tmp("bytes");
        fs::write(dir.join("a.bin"), vec![0u8; 4096]).unwrap();
        fs::write(dir.join("b.bin"), vec![0u8; 8192]).unwrap();
        let out = clean_folder(&dir).unwrap();
        assert_eq!(out.freed_bytes, 4096 + 8192);
        assert_eq!(out.files_removed, 2);
        assert_eq!(out.skipped, 0);
        // Folder itself preserved.
        assert!(dir.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn nested_dirs_counted() {
        let dir = tmp("nested");
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub").join("x"), b"12345").unwrap();
        let out = clean_folder(&dir).unwrap();
        assert_eq!(out.freed_bytes, 5);
        assert_eq!(out.folders_removed, 1);
        assert!(dir.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_path_errors() {
        let r = clean_folder(std::path::Path::new(r"Z:\definitely\not\here"));
        assert!(matches!(r, Err(CleanError::NotFound(_))));
    }

    #[test]
    fn empty_folder_no_op() {
        let dir = tmp("empty");
        let out = clean_folder(&dir).unwrap();
        assert_eq!(out.freed_bytes, 0);
        assert_eq!(out.files_removed, 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn clean_file_works() {
        let dir = tmp("file_clean");
        let f = dir.join("test.bin");
        fs::write(&f, vec![0u8; 100]).unwrap();
        let freed = clean_file(&f).unwrap();
        assert_eq!(freed, 100);
        assert!(!f.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
