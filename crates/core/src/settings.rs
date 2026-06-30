//! Application settings configuration.
//!
//! Handles loading of custom folders, stale days threshold, and model path
//! overrides from a settings.toml file.

use crate::rules::{FolderRule, RuleSet};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Global application settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Number of days before a file is considered stale. Default is 90.
    #[serde(default = "default_stale_days")]
    pub stale_days: u32,

    /// Configuration specific to the LLM agent.
    #[serde(default)]
    pub llm: LlmSettings,

    /// Configuration for the periodic scan scheduler.
    #[serde(default)]
    pub scheduler: SchedulerSettings,

    /// Optional list of folder rules to override the defaults.
    pub folders: Option<Vec<FolderRule>>,

    /// Optional list of path substrings to exclude from scans.
    #[serde(default)]
    pub exclusions: Vec<String>,
}

/// LLM engine settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LlmSettings {
    /// Custom path to the model GGUF file.
    pub model_path: Option<String>,
}

/// Scheduler settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerSettings {
    /// Whether the periodic scheduler is enabled.
    #[serde(default = "default_scheduler_enabled")]
    pub enabled: bool,
    /// Interval between scans in minutes.
    #[serde(default = "default_scheduler_interval")]
    pub interval_mins: u32,
}

fn default_scheduler_enabled() -> bool {
    false
}

fn default_scheduler_interval() -> u32 {
    60
}

impl Default for SchedulerSettings {
    fn default() -> Self {
        Self {
            enabled: default_scheduler_enabled(),
            interval_mins: default_scheduler_interval(),
        }
    }
}

fn default_stale_days() -> u32 {
    90
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            stale_days: default_stale_days(),
            llm: LlmSettings::default(),
            scheduler: SchedulerSettings::default(),
            folders: None,
            exclusions: Vec::new(),
        }
    }
}

impl Settings {
    /// Load settings from a TOML file.
    ///
    /// If the file is missing or invalid, it returns `Settings::default()`
    /// with warnings logged.
    pub fn load(path: &Path) -> Self {
        if let Ok(content) = std::fs::read_to_string(path) {
            toml::from_str(&content).unwrap_or_else(|e| {
                log::warn!("Failed to parse settings TOML: {}. Using default settings.", e);
                Settings::default()
            })
        } else {
            Settings::default()
        }
    }

    /// Save settings to a TOML file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let text = toml::to_string_pretty(self).unwrap_or_default();
        std::fs::write(path, text)
    }

    /// Resolve the RuleSet to use.
    ///
    /// If the TOML has custom folders specified, it returns them.
    /// Otherwise, it falls back to the default ruleset.
    pub fn ruleset(&self) -> RuleSet {
        if let Some(folders) = &self.folders {
            RuleSet {
                folders: folders.clone(),
            }
        } else {
            RuleSet::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_settings_toml() {
        let toml_str = r#"
            stale_days = 45
            [llm]
            model_path = "D:\\custom\\model.gguf"
            [scheduler]
            enabled = true
            interval_mins = 30
        "#;
        let settings: Settings = toml::from_str(toml_str).unwrap();
        assert_eq!(settings.stale_days, 45);
        assert_eq!(settings.llm.model_path.as_deref(), Some("D:\\custom\\model.gguf"));
        assert!(settings.scheduler.enabled);
        assert_eq!(settings.scheduler.interval_mins, 30);
        assert!(settings.folders.is_none());
    }
}
