use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use walkdir::WalkDir;

use crate::util::dirs_home;

pub const DEFAULT_ACTIVE_THRESHOLD_SECS: u64 = 15 * 60;

pub fn active_session_repos(threshold: Duration) -> HashSet<PathBuf> {
    let projects_dir = dirs_home().join(".claude").join("projects");
    let Ok(entries) = fs::read_dir(&projects_dir) else {
        return HashSet::new();
    };

    let cutoff = SystemTime::now().checked_sub(threshold);
    let mut active = HashSet::new();

    for entry in entries.flatten() {
        let slug_dir = entry.path();
        if !slug_dir.is_dir() {
            continue;
        }
        let Some(slug) = slug_dir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        let is_active = match (latest_mtime(&slug_dir), cutoff) {
            (Some(m), Some(c)) => m >= c,
            _ => false,
        };
        if !is_active {
            continue;
        }
        if let Some(repo) = resolve_slug(slug) {
            active.insert(repo);
        }
    }
    active
}

fn latest_mtime(dir: &Path) -> Option<SystemTime> {
    WalkDir::new(dir)
        .max_depth(4)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.metadata().ok())
        .filter_map(|m| m.modified().ok())
        .max()
}

fn resolve_slug(slug: &str) -> Option<PathBuf> {
    let rest = slug.strip_prefix('-')?;
    let parts: Vec<&str> = rest.split('-').collect();
    probe(Path::new("/"), &parts)
}

fn probe(base: &Path, rest: &[&str]) -> Option<PathBuf> {
    if rest.is_empty() {
        return Some(base.to_path_buf());
    }
    for take in (1..=rest.len()).rev() {
        let segment = rest[..take].join("-");
        let candidate = base.join(&segment);
        if candidate.is_dir() {
            if let Some(p) = probe(&candidate, &rest[take..]) {
                return Some(p);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_prefers_longer_segments() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("bet-trackers");
        std::fs::create_dir_all(&nested).unwrap();
        let parts: Vec<&str> = vec!["bet", "trackers"];
        let found = probe(tmp.path(), &parts).unwrap();
        assert_eq!(found, nested);
    }

    #[test]
    fn probe_falls_back_to_split_segments() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("bet").join("trackers");
        std::fs::create_dir_all(&nested).unwrap();
        let parts: Vec<&str> = vec!["bet", "trackers"];
        let found = probe(tmp.path(), &parts).unwrap();
        assert_eq!(found, nested);
    }

    #[test]
    fn resolve_unknown_slug_returns_none() {
        assert!(resolve_slug("-nonexistent-path-xyz-12345").is_none());
    }
}
