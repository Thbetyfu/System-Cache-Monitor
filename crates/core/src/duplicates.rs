//! Duplicate file detection.
//!
//! Optimizes scan time by pre-filtering files by size before computing
//! SHA-256 hashes, avoiding unnecessary I/O for unique files.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

/// A group of duplicate files.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DuplicateGroup {
    /// SHA-256 hash of the files' content.
    pub hash: String,
    /// Size of a single file in bytes.
    pub file_size: u64,
    /// Paths to all identical copies of this file.
    pub file_paths: Vec<PathBuf>,
}

/// Walk the given directories recursively to find duplicate files.
///
/// Files are matched based on exact size, then verified via SHA-256 hash.
/// Empty files (0 bytes) are ignored.
pub fn find_duplicates(directories: &[PathBuf], exclusions: &[String]) -> Vec<DuplicateGroup> {
    let mut size_map: HashMap<u64, Vec<PathBuf>> = HashMap::new();

    // 1. Gather all files and map them by size
    for dir in directories {
        if !dir.exists() {
            continue;
        }
        let entries: Vec<_> = walkdir::WalkDir::new(dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let path_str = e.path().to_string_lossy().to_lowercase();
                !exclusions.iter().any(|ex| {
                    let ex_low = ex.to_lowercase();
                    path_str == ex_low
                        || path_str.starts_with(&format!("{}\\", ex_low))
                        || path_str.starts_with(&format!("{}/", ex_low))
                })
            })
            .collect();

        for entry in entries {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if let Ok(meta) = entry.metadata() {
                let sz = meta.len();
                if sz > 0 {
                    size_map.entry(sz).or_default().push(path.to_path_buf());
                }
            }
        }
    }

    // 2. Filter sizes with potential duplicates (> 1 file)
    let candidate_sizes: Vec<(u64, Vec<PathBuf>)> = size_map
        .into_iter()
        .filter(|(_, paths)| paths.len() > 1)
        .collect();

    // 3. Hash candidate files to identify exact duplicates
    let mut hash_map: HashMap<String, (u64, Vec<PathBuf>)> = HashMap::new();

    for (size, paths) in candidate_sizes {
        for path in paths {
            if let Ok(hash_str) = compute_sha256(&path) {
                let entry = hash_map.entry(hash_str).or_insert((size, Vec::new()));
                // Avoid adding exact duplicate paths if scanned multiple times due to directory overlap
                if !entry.1.contains(&path) {
                    entry.1.push(path);
                }
            }
        }
    }

    // 4. Collect groups that have actual duplicates (> 1 file)
    let mut groups: Vec<DuplicateGroup> = hash_map
        .into_iter()
        .filter(|(_, (_, paths))| paths.len() > 1)
        .map(|(hash, (size, paths))| DuplicateGroup {
            hash,
            file_size: size,
            file_paths: paths,
        })
        .collect();

    // Sort descending by size (largest duplicates first)
    groups.sort_by(|a, b| b.file_size.cmp(&a.file_size));
    groups
}

fn compute_sha256(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 16384];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp_dir(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("ca_dup_test_{}", name));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn find_duplicates_works() {
        let dir = tmp_dir("find");

        let f1 = dir.join("file1.txt");
        let f2 = dir.join("file2.txt");
        let f3 = dir.join("unique.txt");

        fs::write(&f1, "hello duplicate content").unwrap();
        fs::write(&f2, "hello duplicate content").unwrap();
        fs::write(&f3, "unique content").unwrap();

        let groups = find_duplicates(&[dir.clone()], &[]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].file_size, f1.metadata().unwrap().len());
        assert_eq!(groups[0].file_paths.len(), 2);
        assert!(groups[0].file_paths.contains(&f1));
        assert!(groups[0].file_paths.contains(&f2));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_duplicates_skips_exclusions() {
        let dir = tmp_dir("exclusions");

        let f1 = dir.join("file1.txt");
        let f2 = dir.join("file2.txt");

        fs::write(&f1, "hello duplicate content").unwrap();
        fs::write(&f2, "hello duplicate content").unwrap();

        let exclusions = vec![f2.to_string_lossy().to_string()];
        let groups = find_duplicates(&[dir.clone()], &exclusions);
        
        assert_eq!(groups.len(), 0);

        let _ = fs::remove_dir_all(&dir);
    }
}
