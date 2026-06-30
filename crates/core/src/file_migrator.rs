use std::fs;
use std::path::Path;

/// Copy a directory recursively from `src` to `dst`.
pub fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &dest_path)?;
        } else {
            fs::copy(&entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

/// Relocate/Migrate a folder from `src` to `dst`.
/// If the migration fails at any step, it performs a rollback by clearing the destination.
pub fn migrate_folder(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !src.exists() {
        return Ok(());
    }

    // Attempt to copy all files recursively
    if let Err(e) = copy_dir_all(src, dst) {
        // Rollback: delete whatever was partially copied to dst
        let _ = fs::remove_dir_all(dst);
        return Err(e);
    }

    // Verification step: verify we can read/delete the source.
    // If we fail to remove the source (e.g. locked files), we rollback by deleting the dst.
    if let Err(e) = fs::remove_dir_all(src) {
        let _ = fs::remove_dir_all(dst);
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("Source files are locked, rollback triggered: {}", e),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dir_copy_and_migration() {
        let temp_dir = std::env::temp_dir();
        let src_path = temp_dir.join("ca_migration_src_test");
        let dst_path = temp_dir.join("ca_migration_dst_test");

        // Clean up old runs
        let _ = fs::remove_dir_all(&src_path);
        let _ = fs::remove_dir_all(&dst_path);

        // Setup source structure
        fs::create_dir_all(src_path.join("nested")).unwrap();
        fs::write(src_path.join("file1.txt"), "hello").unwrap();
        fs::write(src_path.join("nested/file2.txt"), "world").unwrap();

        // Migrate
        migrate_folder(&src_path, &dst_path).unwrap();

        // Verify source is deleted
        assert!(!src_path.exists());

        // Verify destination has the exact contents
        assert!(dst_path.exists());
        assert_eq!(fs::read_to_string(dst_path.join("file1.txt")).unwrap(), "hello");
        assert_eq!(fs::read_to_string(dst_path.join("nested/file2.txt")).unwrap(), "world");

        // Clean up
        fs::remove_dir_all(&dst_path).unwrap();
    }
}
