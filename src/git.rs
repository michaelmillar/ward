use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

pub fn find_git_repos(root: &Path) -> Vec<PathBuf> {
    let mut repos = Vec::new();
    let mut it = WalkDir::new(root).into_iter();
    loop {
        let entry = match it.next() {
            None => break,
            Some(Err(_)) => continue,
            Some(Ok(e)) => e,
        };
        if !entry.file_type().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" {
            it.skip_current_dir();
            continue;
        }
        if name.starts_with('.') && entry.depth() > 0 {
            it.skip_current_dir();
            continue;
        }
        if entry.path().join(".git").is_dir() {
            repos.push(entry.path().to_path_buf());
            it.skip_current_dir();
        }
    }
    repos
}

pub fn remote_urls(repo: &Path) -> Vec<(String, String)> {
    Command::new("git")
        .args(["remote", "-v"])
        .current_dir(repo)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .map(|text| {
            let mut seen = std::collections::HashSet::new();
            let mut result = Vec::new();
            for line in text.lines() {
                let mut parts = line.split_whitespace();
                let Some(name) = parts.next() else { continue };
                let Some(url) = parts.next() else { continue };
                let key = format!("{name}:{url}");
                if seen.insert(key) {
                    result.push((name.to_string(), url.to_string()));
                }
            }
            result
        })
        .unwrap_or_default()
}

pub fn origin_url(repo: &Path) -> Option<String> {
    Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn last_commit_date(repo: &Path) -> Option<chrono::NaiveDate> {
    Command::new("git")
        .args(["log", "-1", "--format=%aI"])
        .current_dir(repo)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            chrono::DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|dt| dt.date_naive())
        })
}

pub fn first_commit_date(repo: &Path) -> Option<chrono::NaiveDate> {
    Command::new("git")
        .args(["log", "--reverse", "--format=%aI"])
        .current_dir(repo)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout);
            let first = s.lines().next()?.trim().to_string();
            chrono::DateTime::parse_from_rfc3339(&first)
                .ok()
                .map(|dt| dt.date_naive())
        })
}

pub fn head_sha(repo: &Path) -> Option<String> {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn root_commit_sha(repo: &Path) -> Option<String> {
    Command::new("git")
        .args(["rev-list", "--max-parents=0", "HEAD"])
        .current_dir(repo)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let text = String::from_utf8_lossy(&o.stdout).to_string();
            text.lines().next().map(|s| s.trim().to_string())
        })
        .filter(|s| !s.is_empty())
}

pub fn commit_count(repo: &Path) -> u64 {
    Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(repo)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<u64>()
                .ok()
        })
        .unwrap_or(0)
}

pub fn author_count(repo: &Path) -> u64 {
    Command::new("git")
        .args(["log", "--format=%ae"])
        .current_dir(repo)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            let text = String::from_utf8_lossy(&o.stdout).to_string();
            let mut set = std::collections::HashSet::new();
            for line in text.lines() {
                set.insert(line.trim().to_string());
            }
            set.len() as u64
        })
        .unwrap_or(0)
}

pub fn has_uncommitted_changes(repo: &Path) -> Result<bool> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(repo)
        .output()
        .context("Failed to run git status")?;
    let text = String::from_utf8_lossy(&output.stdout);
    let has_tracked_changes = text
        .lines()
        .any(|l| !l.starts_with("?? ") && !l.starts_with("!! ") && !l.trim().is_empty());
    Ok(has_tracked_changes)
}

pub fn untracked_count(repo: &Path) -> Result<(u64, u64)> {
    let output = Command::new("git")
        .args(["status", "--porcelain", "--ignored"])
        .current_dir(repo)
        .output()
        .context("Failed to run git status")?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut untracked = 0u64;
    let mut ignored = 0u64;
    for line in text.lines() {
        if line.starts_with("?? ") {
            untracked += 1;
        } else if line.starts_with("!! ") {
            ignored += 1;
        }
    }
    Ok((untracked, ignored))
}

pub fn stash_count(repo: &Path) -> u64 {
    Command::new("git")
        .args(["stash", "list"])
        .current_dir(repo)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().count() as u64)
        .unwrap_or(0)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BranchStatus {
    pub name: String,
    pub upstream: Option<String>,
    pub ahead: u64,
}

pub fn branches(repo: &Path) -> Vec<BranchStatus> {
    let output = Command::new("git")
        .args([
            "for-each-ref",
            "--format=%(refname:short)|%(upstream:short)|%(upstream:track)",
            "refs/heads",
        ])
        .current_dir(repo)
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    let mut result = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() < 2 {
            continue;
        }
        let name = parts[0].to_string();
        let upstream = if parts[1].is_empty() {
            None
        } else {
            Some(parts[1].to_string())
        };
        let track = parts.get(2).unwrap_or(&"");
        let ahead = parse_ahead(track);
        result.push(BranchStatus {
            name,
            upstream,
            ahead,
        });
    }
    result
}

fn parse_ahead(s: &str) -> u64 {
    let inner = s.trim_start_matches('[').trim_end_matches(']');
    for part in inner.split(", ") {
        if let Some(n) = part.strip_prefix("ahead ") {
            return n.parse().unwrap_or(0);
        }
    }
    0
}

pub fn tag_count(repo: &Path) -> u64 {
    Command::new("git")
        .args(["tag", "--list"])
        .current_dir(repo)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().count() as u64)
        .unwrap_or(0)
}

pub fn worktree_paths(repo: &Path) -> Vec<PathBuf> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo)
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    let mut paths = Vec::new();
    for line in text.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            paths.push(PathBuf::from(path));
        }
    }
    paths
}

pub fn stash_shas(repo: &Path) -> Vec<String> {
    Command::new("git")
        .args(["reflog", "show", "refs/stash", "--format=%H"])
        .current_dir(repo)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

pub fn create_stash_refs(repo: &Path) -> Result<Vec<String>> {
    let shas = stash_shas(repo);
    for (i, sha) in shas.iter().enumerate() {
        let refname = format!("refs/ward-stash/{i}");
        let output = Command::new("git")
            .args(["update-ref", &refname, sha])
            .current_dir(repo)
            .output()
            .context("Failed to create stash ref")?;
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).to_string();
            anyhow::bail!("update-ref failed for {refname}: {err}");
        }
    }
    Ok(shas)
}

pub fn cleanup_stash_refs(repo: &Path) {
    let output = Command::new("git")
        .args(["for-each-ref", "--format=%(refname)", "refs/ward-stash/"])
        .current_dir(repo)
        .output();
    if let Ok(o) = output {
        let text = String::from_utf8_lossy(&o.stdout).to_string();
        for refname in text.lines() {
            let _ = Command::new("git")
                .args(["update-ref", "-d", refname.trim()])
                .current_dir(repo)
                .output();
        }
    }
}

pub fn restore_stash_refs(repo: &Path) -> Result<u64> {
    let output = Command::new("git")
        .args([
            "for-each-ref",
            "--sort=refname",
            "--format=%(refname) %(objectname)",
            "refs/ward-stash/",
        ])
        .current_dir(repo)
        .output()
        .context("Failed to list ward-stash refs")?;
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    let mut count = 0u64;
    for line in text.lines() {
        let mut parts = line.splitn(2, ' ');
        let Some(refname) = parts.next() else {
            continue;
        };
        let Some(sha) = parts.next() else { continue };
        let store = Command::new("git")
            .args(["stash", "store", "-m", "restored by ward", sha.trim()])
            .current_dir(repo)
            .output();
        if store.is_ok() {
            count += 1;
        }
        let _ = Command::new("git")
            .args(["update-ref", "-d", refname.trim()])
            .current_dir(repo)
            .output();
    }
    Ok(count)
}

pub fn has_submodules(repo: &Path) -> bool {
    repo.join(".gitmodules").exists()
}

pub fn submodule_count(repo: &Path) -> u64 {
    if !has_submodules(repo) {
        return 0;
    }
    Command::new("git")
        .args(["submodule", "status"])
        .current_dir(repo)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .count() as u64
        })
        .unwrap_or(0)
}

pub fn has_worktree_config(repo: &Path) -> bool {
    repo.join(".git/config.worktree").exists()
}

pub fn effective_hooks_path(repo: &Path) -> Option<PathBuf> {
    let output = std::process::Command::new("git")
        .args(["config", "--get", "core.hooksPath"])
        .current_dir(repo)
        .output()
        .ok()?;
    if output.status.success() {
        let p = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if p.is_empty() {
            return None;
        }
        let path = if let Some(rest) = p.strip_prefix("~/") {
            if let Ok(home) = std::env::var("HOME") {
                PathBuf::from(format!("{home}/{rest}"))
            } else {
                PathBuf::from(&p)
            }
        } else {
            PathBuf::from(&p)
        };
        if path.is_dir() {
            return Some(path);
        }
    }
    None
}

pub fn custom_hooks(repo: &Path) -> Vec<PathBuf> {
    let hooks_dir = effective_hooks_path(repo)
        .unwrap_or_else(|| repo.join(".git/hooks"));
    if !hooks_dir.is_dir() {
        return Vec::new();
    }
    std::fs::read_dir(&hooks_dir)
        .ok()
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_file() && !p.to_string_lossy().ends_with(".sample"))
                .collect()
        })
        .unwrap_or_default()
}

pub fn normalise_remote_url(url: &str) -> String {
    let s = url.trim();
    let s = s.strip_suffix(".git").unwrap_or(s);
    let s = if let Some(rest) = s.strip_prefix("git@") {
        rest.replacen(':', "/", 1)
    } else {
        s.to_string()
    };
    let s = s.strip_prefix("https://").unwrap_or(&s);
    let s = s.strip_prefix("http://").unwrap_or(s);
    let s = s.strip_prefix("ssh://").unwrap_or(s);
    s.to_lowercase()
}
