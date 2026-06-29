//! Risk classifier — the deterministic "brain" that decides what is safe to
//! delete, move to external storage, or leave alone.
//!
//! This is deliberately rule-based, not a neural net. The decision "is this
//! cache safe to delete" is deterministic given (tier, age, size, type) and
//! should not depend on a language model. The LLM (in the `llm` crate) is only
//! used to *explain* these decisions in natural language — it never decides.

use crate::rules::{CleaningTier, FolderRule};
use crate::scanner::FolderStats;
use serde::{Deserialize, Serialize};

/// Coarse bucket used for row coloring in the UI. Mirrors the old Python
/// green/yellow/red scheme but is derived from a real score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskLevel {
    /// Small / fresh / monitor-only — leave it, no action needed.
    Healthy,
    /// Worth a look; moderate size or getting stale.
    Watch,
    /// Large or stale cache — prime delete candidate.
    Heavy,
    /// Cannot clean (monitor-only) regardless of size.
    Protected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskScore {
    /// 0..=100, higher = more urgent to act.
    pub urgency: u8,
    pub level: RiskLevel,
    /// One-line reason, shown in UI and fed to the LLM as context.
    pub reason: String,
    /// Whether deletion is auto-allowed (cache tier) vs needs confirm.
    pub auto_cleanable: bool,
    /// Whether this is a good candidate to *archive to external* instead of delete.
    pub archive_candidate: bool,
}

/// Size thresholds (bytes). Tuned to match the old Python warning levels.
const WARN_BYTES: u64 = 500 * 1024 * 1024; // 500 MB
const HEAVY_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GB
const STALE_RATIO: f64 = 0.5; // >50% stale files nudges urgency up

pub fn classify(rule: &FolderRule, stats: &FolderStats) -> RiskScore {
    if !stats.exists {
        return RiskScore {
            urgency: 0,
            level: RiskLevel::Healthy,
            reason: "Folder not found.".into(),
            auto_cleanable: false,
            archive_candidate: false,
        };
    }

    if rule.tier == CleaningTier::MonitorOnly {
        return RiskScore {
            urgency: 0,
            level: RiskLevel::Protected,
            reason: format!(
                "{} — monitor only, never auto-deleted.",
                rule.note.clone().unwrap_or_else(|| "Protected".into())
            ),
            auto_cleanable: false,
            // Even protected folders can be archived if huge (e.g. Docker images),
            // but only with explicit confirmation.
            archive_candidate: stats.total_bytes >= HEAVY_BYTES,
        };
    }

    let bytes = stats.total_bytes;
    let stale_ratio = if stats.file_count > 0 {
        stats.stale_file_count as f64 / stats.file_count as f64
    } else {
        0.0
    };

    // Base urgency from size.
    let mut urgency: u8 = if bytes >= HEAVY_BYTES {
        80
    } else if bytes >= WARN_BYTES {
        50
    } else if bytes >= 50 * 1024 * 1024 {
        25
    } else {
        5
    };

    // Stale files raise urgency — old cache is safer to delete than fresh.
    if stale_ratio > STALE_RATIO {
        urgency = urgency.saturating_add(10);
    }

    let level = if bytes >= HEAVY_BYTES {
        RiskLevel::Heavy
    } else if bytes >= WARN_BYTES {
        RiskLevel::Watch
    } else {
        RiskLevel::Healthy
    };

    let reason = build_reason(bytes, stats.stale_file_count, stats.file_count, stale_ratio);

    RiskScore {
        urgency,
        level,
        reason,
        auto_cleanable: rule.tier == CleaningTier::Cache,
        archive_candidate: bytes >= WARN_BYTES && stale_ratio < 0.3,
    }
}

fn build_reason(bytes: u64, stale: u64, total: u64, stale_ratio: f64) -> String {
    let size = crate::scanner::format_bytes(bytes);
    let pct = if total > 0 {
        (stale_ratio * 100.0) as u8
    } else {
        0
    };
    format!("{size}; {stale}/{total} files older than 90d ({pct}%).")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::CleaningTier;
    use std::path::PathBuf;

    fn rule(tier: CleaningTier) -> FolderRule {
        FolderRule {
            name: "t".into(),
            path: PathBuf::from("."),
            tier,
            note: None,
            archive_dest: None,
        }
    }

    #[test]
    fn monitor_only_is_protected() {
        let r = rule(CleaningTier::MonitorOnly);
        let s = FolderStats { exists: true, total_bytes: 5_000_000_000, ..Default::default() };
        let sc = classify(&r, &s);
        assert_eq!(sc.level, RiskLevel::Protected);
        assert!(!sc.auto_cleanable);
        assert!(sc.archive_candidate); // huge protected folder => archive candidate
    }

    #[test]
    fn heavy_cache_is_auto_cleanable() {
        let r = rule(CleaningTier::Cache);
        let s = FolderStats {
            exists: true,
            total_bytes: 3 * 1024 * 1024 * 1024,
            file_count: 100,
            stale_file_count: 80,
            ..Default::default()
        };
        let sc = classify(&r, &s);
        assert_eq!(sc.level, RiskLevel::Heavy);
        assert!(sc.auto_cleanable);
        assert!(sc.urgency > 80);
    }

    #[test]
    fn small_healthy_folder() {
        let r = rule(CleaningTier::Cache);
        let s = FolderStats { exists: true, total_bytes: 1024, file_count: 1, ..Default::default() };
        let sc = classify(&r, &s);
        assert_eq!(sc.level, RiskLevel::Healthy);
        assert!(sc.urgency < 30);
    }

    #[test]
    fn not_found_is_healthy_zero_urgency() {
        let r = rule(CleaningTier::Cache);
        let s = FolderStats { exists: false, ..Default::default() };
        let sc = classify(&r, &s);
        assert_eq!(sc.level, RiskLevel::Healthy);
        assert_eq!(sc.urgency, 0);
        assert!(!sc.auto_cleanable);
    }
}
