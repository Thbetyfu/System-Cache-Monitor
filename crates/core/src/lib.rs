//! Cache-Advisor core: pure logic for scanning, classifying, and archiving.
//!
//! No filesystem mutations happen here. `actions` crate owns destructive ops.
//! This keeps unit tests fast and side-effect free.

pub mod rules;
pub mod scanner;
pub mod classifier;
pub mod archive;
pub mod settings;
pub mod duplicates;
pub mod disk_map;
pub mod env_manager;
pub mod junction_manager;
pub mod file_migrator;

pub use rules::{FolderRule, RuleSet, CleaningTier};
pub use scanner::{scan_folder, FolderStats, ScanResult};
pub use classifier::{RiskScore, classify, RiskLevel};
pub use archive::{ArchivePlan, ArchiveEntry};
pub use settings::Settings;
pub use duplicates::{find_duplicates, DuplicateGroup};
pub use disk_map::{scan_drive, DiskNode};
pub use env_manager::{is_admin, set_user_env, unset_user_env, get_user_env};
pub use junction_manager::{is_junction, create_junction, delete_junction, is_process_running};
pub use file_migrator::{copy_dir_all, migrate_folder};
