use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskNode {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub is_dir: bool,
    pub children: Vec<DiskNode>,
}

/// Recursively scan a path, summing sizes, and prune nodes < min_bytes.
/// Uses Rayon parallel iterator to traverse subdirectories concurrently.
pub fn scan_drive(root_path: &Path, min_bytes: u64) -> Option<DiskNode> {
    let meta = std::fs::symlink_metadata(root_path).ok()?;
    let is_dir = meta.is_dir();

    if !is_dir {
        let size = meta.len();
        if size >= min_bytes {
            return Some(DiskNode {
                name: root_path.file_name()?.to_string_lossy().into_owned(),
                path: root_path.to_path_buf(),
                size,
                is_dir: false,
                children: Vec::new(),
            });
        }
        return None;
    }

    // It's a directory. Read its children.
    let read_dir = std::fs::read_dir(root_path).ok()?;
    let mut paths = Vec::new();
    for entry in read_dir {
        if let Ok(entry) = entry {
            paths.push(entry.path());
        }
    }

    // Process children in parallel using Rayon!
    use rayon::prelude::*;
    let mut children: Vec<DiskNode> = paths
        .into_par_iter()
        .filter_map(|p| scan_drive(&p, min_bytes))
        .collect();

    // Sort by size descending
    children.sort_by(|a, b| b.size.cmp(&a.size));

    let size: u64 = children.iter().map(|c| c.size).sum();

    if size >= min_bytes {
        let name = root_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| root_path.to_string_lossy().into_owned());
        Some(DiskNode {
            name,
            path: root_path.to_path_buf(),
            size,
            is_dir: true,
            children,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn w(p: &Path, name: &str, data: &[u8]) {
        fs::create_dir_all(p).unwrap();
        fs::write(p.join(name), data).unwrap();
    }

    #[test]
    fn scan_drive_pruning_and_sorting() {
        let root = std::env::temp_dir().join("ca_disk_map_test");
        let _ = fs::remove_dir_all(&root);

        // Create structure:
        // root/
        //   large_dir/
        //     f1.bin (10 MB)
        //     f2.bin (20 MB)
        //   small_dir/
        //     f3.bin (1 KB)
        w(&root.join("large_dir"), "f1.bin", &vec![0u8; 10 * 1024 * 1024]);
        w(&root.join("large_dir"), "f2.bin", &vec![0u8; 20 * 1024 * 1024]);
        w(&root.join("small_dir"), "f3.bin", &vec![0u8; 1024]);

        // Pruning threshold: 5 MB (5 * 1024 * 1024 bytes)
        let tree = scan_drive(&root, 5 * 1024 * 1024).expect("should find heavy folders");

        assert_eq!(tree.is_dir, true);
        // Size of root is sum of large_dir (since small_dir is pruned < 5MB)
        assert!(tree.size >= 30 * 1024 * 1024);

        // Children should only contain large_dir
        assert_eq!(tree.children.len(), 1);
        let large_dir = &tree.children[0];
        assert_eq!(large_dir.name, "large_dir");

        // large_dir children should contain f2.bin and f1.bin, sorted by size desc!
        assert_eq!(large_dir.children.len(), 2);
        assert_eq!(large_dir.children[0].name, "f2.bin");
        assert_eq!(large_dir.children[0].size, 20 * 1024 * 1024);
        assert_eq!(large_dir.children[1].name, "f1.bin");
        assert_eq!(large_dir.children[1].size, 10 * 1024 * 1024);

        let _ = fs::remove_dir_all(&root);
    }
}
