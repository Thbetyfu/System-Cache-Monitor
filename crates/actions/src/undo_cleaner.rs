use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::{Context, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecycledEntry {
    pub original: PathBuf,
    pub recycled: PathBuf,
    pub bytes: u64,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanSession {
    pub session_id: String,
    pub timestamp_secs: u64,
    pub freed_bytes: u64,
    pub entries: Vec<RecycledEntry>,
}

pub const SESSION_MANIFEST_NAME: &str = "clean-session-manifest.json";

/// Move a file or directory tree to the recycle bin location.
/// Returns bytes moved.
fn recycle_item(src: &Path, dst: &Path, is_dir: bool) -> Result<u64> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).context("create recycle target parent")?;
    }
    
    let size = if is_dir {
        dir_tree_size(src)
    } else {
        fs::symlink_metadata(src).map(|m| m.len()).unwrap_or(0)
    };

    // Attempt rename (atomic on same volume)
    if fs::rename(src, dst).is_ok() {
        return Ok(size);
    }

    // Fallback: copy then delete if across volumes
    if is_dir {
        copy_dir_all(src, dst)?;
        let _ = fs::remove_dir_all(src);
    } else {
        fs::copy(src, dst)?;
        let _ = fs::remove_file(src);
    }
    
    Ok(size)
}

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

fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dst.join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}

/// Delete everything inside `path` and move them to `recycle_root` as a session.
pub fn clean_folder_to_recycle_bin(
    path: &Path,
    recycle_root: &Path,
) -> Result<CleanSession> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    
    let session_id = format!("clean_session_{}", now);
    let session_dir = recycle_root.join(&session_id);
    fs::create_dir_all(&session_dir)?;

    let mut entries = Vec::new();
    let mut freed_bytes: u64 = 0;
    
    let read_entries = fs::read_dir(path)?;
    for entry in read_entries.flatten() {
        let p = entry.path();
        let name = entry.file_name();
        let meta = match fs::symlink_metadata(&p) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let is_dir = meta.is_dir();
        let dst = session_dir.join(&name);

        match recycle_item(&p, &dst, is_dir) {
            Ok(size) => {
                entries.push(RecycledEntry {
                    original: p,
                    recycled: dst,
                    bytes: size,
                    is_dir,
                });
                freed_bytes += size;
            }
            Err(e) => {
                log::warn!("Failed to move to recycle bin: {}", e);
            }
        }
    }

    let session = CleanSession {
        session_id,
        timestamp_secs: now,
        freed_bytes,
        entries,
    };

    // Save manifest inside session_dir
    let manifest_path = session_dir.join(SESSION_MANIFEST_NAME);
    let s = serde_json::to_string_pretty(&session)?;
    fs::write(manifest_path, s)?;

    Ok(session)
}

/// Move a single file to `recycle_root` as a session.
pub fn clean_file_to_recycle_bin(
    path: &Path,
    recycle_root: &Path,
) -> Result<CleanSession> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    
    let session_id = format!("clean_session_{}", now);
    let session_dir = recycle_root.join(&session_id);
    fs::create_dir_all(&session_dir)?;

    let name = path.file_name().context("no filename")?;
    let dst = session_dir.join(name);
    
    let size = recycle_item(path, &dst, false)?;

    let session = CleanSession {
        session_id,
        timestamp_secs: now,
        freed_bytes: size,
        entries: vec![RecycledEntry {
            original: path.to_path_buf(),
            recycled: dst,
            bytes: size,
            is_dir: false,
        }],
    };

    // Save manifest inside session_dir
    let manifest_path = session_dir.join(SESSION_MANIFEST_NAME);
    let s = serde_json::to_string_pretty(&session)?;
    fs::write(manifest_path, s)?;

    Ok(session)
}

/// Restore a clean session back to its original location using the manifest file.
pub fn restore_clean_session(manifest_path: &Path) -> Result<()> {
    let content = fs::read_to_string(manifest_path)?;
    let session: CleanSession = serde_json::from_str(&content)?;

    for entry in &session.entries {
        if !entry.recycled.exists() {
            continue;
        }
        if let Some(parent) = entry.original.parent() {
            let _ = fs::create_dir_all(parent);
        }
        
        // Attempt rename
        if fs::rename(&entry.recycled, &entry.original).is_err() {
            // Fallback
            if entry.is_dir {
                copy_dir_all(&entry.recycled, &entry.original)?;
                let _ = fs::remove_dir_all(&entry.recycled);
            } else {
                fs::copy(&entry.recycled, &entry.original)?;
                let _ = fs::remove_file(&entry.recycled);
            }
        }
    }

    // Delete session dir and manifest
    if let Some(session_dir) = manifest_path.parent() {
        let _ = fs::remove_dir_all(session_dir);
    }
    Ok(())
}

/// Permanently delete a clean session from the Recycle Bin.
pub fn purge_clean_session(manifest_path: &Path) -> Result<()> {
    if let Some(session_dir) = manifest_path.parent() {
        fs::remove_dir_all(session_dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(p: &Path, name: &str, data: &[u8]) {
        fs::create_dir_all(p).unwrap();
        fs::write(p.join(name), data).unwrap();
    }

    #[test]
    fn clean_to_recycle_and_restore_roundtrip() {
        let root = std::env::temp_dir().join("ca_undo_clean_test");
        let _ = fs::remove_dir_all(&root);

        let src = root.join("src");
        let rec = root.join("recycle");

        w(&src, "f1.bin", b"data1");
        w(&src.join("sub"), "f2.bin", b"data222");

        // Clean folder to recycle bin
        let session = clean_folder_to_recycle_bin(&src, &rec).unwrap();
        assert_eq!(session.freed_bytes, 12);
        assert!(!src.join("f1.bin").exists());
        assert!(!src.join("sub").join("f2.bin").exists());

        // Verify manifest exists in recycle
        let manifest = rec.join(&session.session_id).join(SESSION_MANIFEST_NAME);
        assert!(manifest.exists());

        // Restore session
        restore_clean_session(&manifest).unwrap();
        assert!(src.join("f1.bin").exists());
        assert!(src.join("sub").join("f2.bin").exists());
        // Manifest directory deleted
        assert!(!rec.join(&session.session_id).exists());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn clean_to_recycle_and_purge() {
        let root = std::env::temp_dir().join("ca_undo_clean_purge_test");
        let _ = fs::remove_dir_all(&root);

        let src = root.join("src");
        let rec = root.join("recycle");

        w(&src, "f1.bin", b"data1");

        let session = clean_folder_to_recycle_bin(&src, &rec).unwrap();
        let manifest = rec.join(&session.session_id).join(SESSION_MANIFEST_NAME);

        // Purge
        purge_clean_session(&manifest).unwrap();
        assert!(!rec.join(&session.session_id).exists());
        assert!(!src.join("f1.bin").exists());

        let _ = fs::remove_dir_all(&root);
    }
}
