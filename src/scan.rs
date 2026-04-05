use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.1} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

pub fn dir_size(path: &Path) -> u64 {
    WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

pub fn find_git_repos(root: &Path) -> Vec<PathBuf> {
    let mut repos = Vec::new();
    let walker = WalkDir::new(root).into_iter();
    for entry in walker.filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        if name == ".git" {
            return false;
        }
        !name.starts_with('.') || e.depth() == 0
    }) {
        let Ok(entry) = entry else { continue };
        if entry.file_type().is_dir() && entry.path().join(".git").is_dir() {
            repos.push(entry.path().to_path_buf());
        }
    }
    repos
}

pub fn git_remote_url(repo: &Path) -> Option<String> {
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

pub fn has_uncommitted_changes(repo: &Path) -> Result<bool> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(repo)
        .output()
        .context("Failed to run git status")?;
    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

pub fn all_branches_pushed(repo: &Path) -> Result<bool> {
    let output = Command::new("git")
        .args(["branch", "-v", "--no-color"])
        .current_dir(repo)
        .output()
        .context("Failed to run git branch")?;
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if line.contains("[ahead") {
            return Ok(false);
        }
    }
    Ok(true)
}

pub fn default_projects_path() -> PathBuf {
    dirs_home().join("projects")
}

pub fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

pub fn reap_archives_dir() -> PathBuf {
    dirs_home().join(".reap/archives")
}
