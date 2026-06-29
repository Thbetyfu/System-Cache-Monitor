//! Folder scanner. Walks a directory, sums real file sizes (not item counts),
//! and gathers age stats so the classifier can score risk.
//!
//! The original Python code counted `freed += 1` per item, which reported a
//! meaningless number. Here size accounting is byte-accurate end to end.

use crate::rules::FolderRule;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FolderStats {
    pub total_bytes: u64,
    pub file_count: u64,
    /// Largest single file size, in bytes.
    pub largest_file: u64,
    /// Oldest modification time seen, as Unix seconds (0 if unknown).
    pub oldest_mtime_secs: i64,
    /// Newest modification time seen, as Unix seconds (0 if unknown).
    pub newest_mtime_secs: i64,
    /// Number of files older than the "stale" threshold (90d).
    pub stale_file_count: u64,
    pub exists: bool,
}

impl FolderStats {
    pub fn human_size(&self) -> String {
        format_bytes(self.total_bytes)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub rule: FolderRule,
    pub stats: FolderStats,
}

/// Scan a single folder rule. Never panics; missing folders return exists=false.
pub fn scan_folder(rule: &FolderRule, stale_days: u32) -> ScanResult {
    let stats = if rule.path.exists() {
        walk_stats(&rule.path, stale_days)
    } else {
        FolderStats { exists: false, ..Default::default() }
    };
    ScanResult { rule: rule.clone(), stats }
}

/// Scan many rules in parallel (rayon thread pool).
pub fn scan_all(rules: &[FolderRule], stale_days: u32) -> Vec<ScanResult> {
    rules.par_iter().map(|r| scan_folder(r, stale_days)).collect()
}

fn walk_stats(root: &Path, stale_days: u32) -> FolderStats {
    let total_bytes = AtomicU64::new(0);
    let file_count = AtomicU64::new(0);
    let largest_file = AtomicU64::new(0);
    let oldest = AtomicU64::new(0); // 0 sentinel = unset
    let newest = AtomicU64::new(0);
    let stale = AtomicU64::new(0);

    let stale_secs = (stale_days as i64) * 24 * 3600;

    let now_secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let entries: Vec<_> = match walkdir::WalkDir::new(root).into_iter().collect() {
        // Collect first so rayon can iterate owned items in parallel.
        v => v,
    };

    entries.par_iter().for_each(|res| {
        let Ok(entry) = res else { return };
        let Ok(ft) = entry.metadata() else { return };
        if !ft.is_file() {
            return;
        }
        let sz = ft.len();
        file_count.fetch_add(1, Ordering::Relaxed);
        total_bytes.fetch_add(sz, Ordering::Relaxed);

        // largest_file
        let mut cur = largest_file.load(Ordering::Relaxed);
        while sz > cur {
            match largest_file.compare_exchange_weak(cur, sz, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => break,
                Err(now) => cur = now,
            }
        }

        if let Ok(mtime) = ft.modified() {
            if let Ok(dur) = mtime.duration_since(SystemTime::UNIX_EPOCH) {
                let s = dur.as_secs() as i64;
                atomic_min(&oldest, s);
                atomic_max(&newest, s);
                if now_secs != 0 && now_secs - s > stale_secs {
                    stale.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    });

    FolderStats {
        total_bytes: total_bytes.load(Ordering::Relaxed),
        file_count: file_count.load(Ordering::Relaxed),
        largest_file: largest_file.load(Ordering::Relaxed),
        oldest_mtime_secs: atomic_load_signed(&oldest),
        newest_mtime_secs: atomic_load_signed(&newest),
        stale_file_count: stale.load(Ordering::Relaxed),
        exists: true,
    }
}

fn atomic_min(slot: &AtomicU64, v: i64) {
    if v <= 0 {
        return;
    }
    let vu = v as u64;
    let mut cur = slot.load(Ordering::Relaxed);
    if cur == 0 {
        // try set
        let _ = slot.compare_exchange(0, vu, Ordering::Relaxed, Ordering::Relaxed);
        return;
    }
    while vu < cur {
        match slot.compare_exchange_weak(cur, vu, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(now) => cur = now,
        }
    }
}

fn atomic_max(slot: &AtomicU64, v: i64) {
    if v <= 0 {
        return;
    }
    let vu = v as u64;
    let mut cur = slot.load(Ordering::Relaxed);
    while vu > cur {
        match slot.compare_exchange_weak(cur, vu, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(now) => cur = now,
        }
    }
}

/// Treat sentinel 0 as "unknown" -> 0; else return as signed.
fn atomic_load_signed(slot: &AtomicU64) -> i64 {
    slot.load(Ordering::Relaxed) as i64
}

/// Format a byte count into a human-readable string (binary units).
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{:.2} {}", size, UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("ca_test_{}", name));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn byte_counter_is_accurate_not_item_count() {
        // Regression for the Python bug: we must report BYTES, not item count.
        let dir = tmp("bytes");
        fs::write(dir.join("a.bin"), vec![0u8; 1024]).unwrap();
        fs::write(dir.join("b.bin"), vec![0u8; 2048]).unwrap();
        let stats = walk_stats(&dir, 90);
        assert_eq!(stats.total_bytes, 1024 + 2048);
        assert_eq!(stats.file_count, 2);
        assert_eq!(stats.largest_file, 2048);
        assert!(stats.exists);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_folder_reports_not_found() {
        let rule = FolderRule {
            name: "x".into(),
            path: PathBuf::from(r"Z:\nope\not\real\path"),
            tier: crate::rules::CleaningTier::Cache,
            note: None,
            archive_dest: None,
        };
        let res = scan_folder(&rule, 90);
        assert!(!res.stats.exists);
        assert_eq!(res.stats.total_bytes, 0);
    }

    #[test]
    fn nested_dirs_summed() {
        let dir = tmp("nested");
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("top.txt"), b"hello").unwrap();
        fs::write(dir.join("sub").join("deep.txt"), b"world!").unwrap();
        let stats = walk_stats(&dir, 90);
        assert_eq!(stats.total_bytes, (b"hello".len() + b"world!".len()) as u64);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn format_bytes_units() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2.00 KB");
        assert!(format_bytes(1024 * 1024 * 5).starts_with("5.00 MB"));
    }
}
