use std::path::Path;
use std::process::Command;

/// Check if a path exists and is an NTFS Directory Junction or Symlink.
pub fn is_junction(path: &Path) -> bool {
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        meta.file_type().is_symlink()
    } else {
        false
    }
}

/// Create an NTFS Directory Junction at `link` pointing to `target`.
pub fn create_junction(link: &Path, target: &Path) -> std::io::Result<bool> {
    #[cfg(target_os = "windows")]
    {
        // If the link folder already exists as a normal folder, we must remove it first.
        // But caller should handle file migration and removal.
        let output = Command::new("cmd")
            .args(&[
                "/c",
                "mklink",
                "/j",
                &link.to_string_lossy(),
                &target.to_string_lossy(),
            ])
            .output()?;
        Ok(output.status.success())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (link, target);
        Ok(false)
    }
}

/// Delete an NTFS Directory Junction safely (deletes only the link, not the target contents).
pub fn delete_junction(link: &Path) -> std::io::Result<bool> {
    if is_junction(link) {
        std::fs::remove_dir(link)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Check if a specific Windows process is currently running (using tasklist).
pub fn is_process_running(process_name: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        if let Ok(output) = Command::new("tasklist")
            .args(&["/FI", &format!("IMAGENAME eq {}", process_name)])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.contains(process_name)
        } else {
            false
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = process_name;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_junction_creation_and_deletion() {
        #[cfg(target_os = "windows")]
        {
            let temp_dir = std::env::temp_dir();
            let target_path = temp_dir.join("ca_junction_target_test");
            let link_path = temp_dir.join("ca_junction_link_test");

            // Clean up old runs
            let _ = fs::remove_dir(&target_path);
            let _ = delete_junction(&link_path);
            let _ = fs::remove_dir(&link_path);

            // Create target folder and a test file in it
            fs::create_dir_all(&target_path).unwrap();
            let test_file = target_path.join("test.txt");
            fs::write(&test_file, "Junction test data").unwrap();

            // Create junction
            let success = create_junction(&link_path, &target_path).unwrap();
            assert!(success, "Junction creation failed");

            // Verify it is recognized as junction
            assert!(is_junction(&link_path));

            // Verify we can read the file through the link
            let link_file = link_path.join("test.txt");
            assert!(link_file.exists());
            let content = fs::read_to_string(&link_file).unwrap();
            assert_eq!(content, "Junction test data");

            // Delete junction
            let deleted = delete_junction(&link_path).unwrap();
            assert!(deleted);

            // Verify link is gone, but target contents STILL exist (safety test!)
            assert!(!link_path.exists());
            assert!(target_path.exists());
            assert!(test_file.exists());

            // Clean up target
            fs::remove_dir_all(&target_path).unwrap();
        }
    }

    #[test]
    fn test_process_running_check() {
        // Should run without crash
        let _ = is_process_running("explorer.exe");
    }
}
