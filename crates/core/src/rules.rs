//! Folder rules — migrated from the old Python `FOLDERS_TO_MONITOR` list.
//!
//! Each rule describes one monitored location, whether it is safe to clean,
//! and how aggressively we may touch it. These are defaults; the app can load
//! overrides from a TOML settings file (see `RuleSet::load`).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// How aggressively a folder may be cleaned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleaningTier {
    /// Never delete. Only report size. (e.g. Docker data)
    MonitorOnly,
    /// Safe to wipe contents; app regenerates them. (e.g. Temp, caches)
    Cache,
    /// Needs explicit confirmation each time. (e.g. build outputs)
    Cautious,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderRule {
    pub name: String,
    pub path: PathBuf,
    pub tier: CleaningTier,
    /// Short human note shown in the UI explaining what this is.
    pub note: Option<String>,
    /// Custom path where this folder should be archived.
    #[serde(default)]
    pub archive_dest: Option<PathBuf>,
}

impl FolderRule {
    /// True when contents may be deleted without per-item confirmation.
    pub fn is_cache(&self) -> bool {
        matches!(self.tier, CleaningTier::Cache)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleSet {
    pub folders: Vec<FolderRule>,
}

impl Default for RuleSet {
    fn default() -> Self {
        use CleaningTier::*;
        let env = std::env::var;

        // Mirrors the original FOLDERS_TO_MONITOR list in cache_monitor.py.
        // Windows-only paths; on other OSes these simply won't exist and the
        // scanner reports them as "not found".
        let folders = vec![
            FolderRule {
                name: "User Temp Files".into(),
                path: PathBuf::from(env("TEMP").unwrap_or_else(|_| r"C:\Windows\Temp".into())),
                tier: Cache,
                note: Some("Per-user temp; safe to wipe.".into()),
                archive_dest: None,
            },
            FolderRule {
                name: "Windows Temp".into(),
                path: PathBuf::from(r"C:\Windows\Temp"),
                tier: Cache,
                note: Some("System temp; may need admin.".into()),
                archive_dest: None,
            },
            FolderRule {
                name: "VS Code Workspace".into(),
                path: dirs_appdata(r"%APPDATA%\Code\User\workspaceStorage"),
                tier: Cache,
                note: Some("VS Code workspace storage; safe to wipe.".into()),
                archive_dest: None,
            },
            FolderRule {
                name: "VS Code Cached Data".into(),
                path: dirs_appdata(r"%APPDATA%\Code\CachedData"),
                tier: Cache,
                note: Some("VS Code cached data.".into()),
                archive_dest: None,
            },
            FolderRule {
                name: "VS Code Extension VSIXs".into(),
                path: dirs_appdata(r"%APPDATA%\Code\CachedExtensionVSIXs"),
                tier: Cache,
                note: Some("Downloaded extension installers.".into()),
                archive_dest: None,
            },
            FolderRule {
                name: "CapCut Cache".into(),
                path: dirs_appdata(r"%LOCALAPPDATA%\CapCut\User Data\Cache"),
                tier: Cache,
                note: Some("CapCut editor cache.".into()),
                archive_dest: None,
            },
            FolderRule {
                name: "CapCut Pre-Render".into(),
                path: dirs_appdata(r"%LOCALAPPDATA%\CapCut\segmentPrerenderCache"),
                tier: Cache,
                note: Some("CapCut pre-rendered segments.".into()),
                archive_dest: None,
            },
            FolderRule {
                name: "Docker Data".into(),
                path: dirs_appdata(r"%LOCALAPPDATA%\Docker"),
                tier: MonitorOnly,
                note: Some("Docker state; NEVER auto-delete.".into()),
                archive_dest: None,
            },
            FolderRule {
                name: "Pip Cache (Drive D)".into(),
                path: PathBuf::from(r"D:\.pip_cache"),
                tier: Cache,
                note: Some("pip download cache; safe to wipe.".into()),
                archive_dest: None,
            },
            FolderRule {
                name: "Dev Tools (Drive D)".into(),
                path: PathBuf::from(r"D:\.dev_tools"),
                tier: MonitorOnly,
                note: Some("Dev tool data; monitor only.".into()),
                archive_dest: None,
            },
        ];
        Self { folders }
    }
}

impl RuleSet {
    /// Load rules from a TOML file. Falls back to defaults on any error.
    pub fn load(path: &std::path::Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).unwrap_or_default(),
            Err(_) => RuleSet::default(),
        }
    }
}

/// Expand a small subset of Windows env-var placeholders. We avoid pulling a
/// `shellexpand`/`dirs` crate just for this — keeps the core dependency-free
/// of platform-specific helpers.
fn dirs_appdata(template: &str) -> PathBuf {
    let mut out = template.to_string();
    for (var, val) in [("APPDATA", "APPDATA"), ("LOCALAPPDATA", "LOCALAPPDATA")] {
        if let Ok(v) = std::env::var(val) {
            out = out.replace(&format!("%{}%", var), &v);
        }
    }
    PathBuf::from(out)
}
