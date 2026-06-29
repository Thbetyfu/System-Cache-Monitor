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

pub use rules::{FolderRule, RuleSet, CleaningTier};
pub use scanner::{scan_folder, FolderStats, ScanResult};
pub use classifier::{RiskScore, classify, RiskLevel};
pub use archive::{ArchivePlan, ArchiveEntry};
pub use settings::Settings;
pub use duplicates::{find_duplicates, DuplicateGroup};
