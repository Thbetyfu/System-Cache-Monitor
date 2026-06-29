//! Archive planning types. The actual move (and undo) lives in `actions`.
//! Keeping the plan separate means the UI can show a preview and the user can
//! edit/confirm before anything touches the filesystem.

use crate::classifier::RiskScore;
use crate::rules::FolderRule;
use crate::scanner::FolderStats;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A single folder selected for archiving to external storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveEntry {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub bytes: u64,
    pub reason: String,
}

/// A user-confirmed set of folders to move to an external drive.
/// Built from scan results + classifier output, then handed to `actions::archiver`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArchivePlan {
    pub entries: Vec<ArchiveEntry>,
    pub external_root: PathBuf,
}

impl ArchivePlan {
    pub fn total_bytes(&self) -> u64 {
        self.entries.iter().map(|e| e.bytes).sum()
    }

    /// Suggest a plan from scan results. Only includes folders flagged as
    /// archive candidates by the classifier. The UI lets the user deselect.
    pub fn suggest(
        results: &[crate::scanner::ScanResult],
        scores: &[crate::classifier::RiskScore],
        external_root: &std::path::Path,
    ) -> Self {
        let mut entries = Vec::new();
        for (res, score) in results.iter().zip(scores.iter()) {
            if !score.archive_candidate {
                continue;
            }
            let dest = if let Some(ref custom_dest) = res.rule.archive_dest {
                custom_dest.clone()
            } else {
                external_root.join("cache-archive").join(res.rule.name.clone())
            };
            entries.push(ArchiveEntry {
                source: res.rule.path.clone(),
                destination: dest,
                bytes: res.stats.total_bytes,
                reason: score.reason.clone(),
            });
        }
        Self {
            entries,
            external_root: external_root.to_path_buf(),
        }
    }
}

/// Iterate scan results paired with their scores for UI convenience.
pub fn pairs<'a>(
    results: &'a [crate::scanner::ScanResult],
    scores: &'a [crate::classifier::RiskScore],
) -> impl Iterator<Item = (&'a FolderRule, &'a FolderStats, &'a RiskScore)> {
    results.iter().zip(scores.iter()).map(|(r, s)| (&r.rule, &r.stats, s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn plan_sums_bytes() {
        let plan = ArchivePlan {
            external_root: PathBuf::from("E:/"),
            entries: vec![
                ArchiveEntry {
                    source: PathBuf::from("a"),
                    destination: PathBuf::from("E:/cache-archive/a"),
                    bytes: 100,
                    reason: "".into(),
                },
                ArchiveEntry {
                    source: PathBuf::from("b"),
                    destination: PathBuf::from("E:/cache-archive/b"),
                    bytes: 250,
                    reason: "".into(),
                },
            ],
        };
        assert_eq!(plan.total_bytes(), 350);
    }
}
