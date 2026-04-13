use anyhow::Result;
use chrono::Utc;
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::config::Thresholds;
use crate::git;
use crate::util::{dir_size, format_size};

#[derive(Clone, Serialize, Deserialize)]
pub struct Assessment {
    pub path: PathBuf,
    pub size: u64,
    pub last_commit: Option<chrono::NaiveDate>,
    pub first_commit: Option<chrono::NaiveDate>,
    pub head_sha: Option<String>,
    pub commit_count: u64,
    pub author_count: u64,
    pub origin_url: Option<String>,
    pub remotes: Vec<(String, String)>,
    pub branches: Vec<git::BranchStatus>,
    pub tag_count: u64,
    pub stash_count: u64,
    pub untracked: u64,
    pub ignored: u64,
    pub dirty: bool,
    pub worktree_count: u64,
    pub submodule_count: u64,
    pub verdict: Verdict,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    Archive,
    Prototype,
    Worktree,
    HasLocalWork,
    KeepAsIs,
    NoRemote,
}

impl Verdict {
    pub fn label(&self) -> colored::ColoredString {
        match self {
            Verdict::Archive => "ARCHIVE".green().bold(),
            Verdict::Prototype => "PROTOTYPE".cyan().bold(),
            Verdict::Worktree => "WORKTREE".blue().bold(),
            Verdict::HasLocalWork => "LOCAL-WORK".yellow().bold(),
            Verdict::KeepAsIs => "KEEP".white().bold(),
            Verdict::NoRemote => "NO-REMOTE".magenta().bold(),
        }
    }
}

pub fn assess_repo(path: &Path, thresholds: &Thresholds) -> Result<Assessment> {
    let size = dir_size(path);
    let last_commit = git::last_commit_date(path);
    let first_commit = git::first_commit_date(path);
    let head_sha = git::head_sha(path);
    let commit_count = git::commit_count(path);
    let author_count = git::author_count(path);
    let origin_url = git::origin_url(path);
    let remotes = git::remote_urls(path);
    let branches = git::branches(path);
    let tag_count = git::tag_count(path);
    let stash_count = git::stash_count(path);
    let (untracked, ignored) = git::untracked_count(path).unwrap_or((0, 0));
    let dirty = git::has_uncommitted_changes(path).unwrap_or(true);
    let worktree_count = git::worktree_paths(path).len().saturating_sub(1) as u64;
    let submodule_count = git::submodule_count(path);

    let all_pushed = branches
        .iter()
        .all(|b| b.upstream.is_some() && b.ahead == 0);
    let has_local_only = branches.iter().any(|b| b.upstream.is_none());
    let has_ahead = branches.iter().any(|b| b.ahead > 0);

    let age_days = last_commit.map(|d| (Utc::now().date_naive() - d).num_days());
    let lifetime_days = match (first_commit, last_commit) {
        (Some(f), Some(l)) => Some((l - f).num_days()),
        _ => None,
    };

    let verdict = classify(
        &RepoFacts {
            has_remote: origin_url.is_some(),
            all_pushed,
            has_local_only,
            has_ahead,
            dirty,
            commit_count,
            author_count,
            lifetime_days,
            age_days,
        },
        thresholds,
    );

    Ok(Assessment {
        path: path.to_path_buf(),
        size,
        last_commit,
        first_commit,
        head_sha,
        commit_count,
        author_count,
        origin_url,
        remotes,
        branches,
        tag_count,
        stash_count,
        untracked,
        ignored,
        dirty,
        worktree_count,
        submodule_count,
        verdict,
    })
}

struct RepoFacts {
    has_remote: bool,
    all_pushed: bool,
    has_local_only: bool,
    has_ahead: bool,
    dirty: bool,
    commit_count: u64,
    author_count: u64,
    lifetime_days: Option<i64>,
    age_days: Option<i64>,
}

fn classify(f: &RepoFacts, t: &Thresholds) -> Verdict {
    let is_prototype = |with_commits_lower_bound: bool| {
        let has_min_commits = if with_commits_lower_bound {
            f.commit_count > 0
        } else {
            true
        };
        has_min_commits
            && f.commit_count < t.prototype_max_commits
            && f.author_count <= t.prototype_max_authors
            && f.lifetime_days
                .map(|d| d < t.prototype_max_lifetime_days)
                .unwrap_or(false)
    };

    if f.dirty {
        return Verdict::HasLocalWork;
    }
    if !f.has_remote {
        if is_prototype(true) {
            return Verdict::Prototype;
        }
        return Verdict::NoRemote;
    }
    if f.has_ahead || f.has_local_only {
        return Verdict::HasLocalWork;
    }
    if f.all_pushed {
        let old = f
            .age_days
            .map(|d| d > t.archive_stale_days)
            .unwrap_or(false);
        if is_prototype(false) {
            return Verdict::Prototype;
        }
        if old {
            return Verdict::Archive;
        }
        return Verdict::KeepAsIs;
    }
    Verdict::HasLocalWork
}

pub fn print_safety_proof(a: &Assessment) {
    let remote_line = match a.origin_url.as_deref() {
        Some(u) => u.to_string(),
        None => "none".to_string(),
    };
    let local_only: Vec<_> = a
        .branches
        .iter()
        .filter(|b| b.upstream.is_none())
        .map(|b| b.name.clone())
        .collect();
    let ahead_branches: Vec<_> = a
        .branches
        .iter()
        .filter(|b| b.ahead > 0)
        .map(|b| format!("{} (+{} ahead)", b.name, b.ahead))
        .collect();

    println!("    {}", "Safety proof".bold());
    println!("      remote           {}", remote_line.dimmed());
    println!("      head             {}", short_sha(&a.head_sha).dimmed());
    println!(
        "      commits          {} across {} author(s)",
        a.commit_count, a.author_count
    );
    println!(
        "      branches         {} ({} local-only, {} ahead)",
        a.branches.len(),
        local_only.len(),
        ahead_branches.len()
    );
    if !local_only.is_empty() {
        println!("      local-only refs  {}", local_only.join(", ").yellow());
    }
    if !ahead_branches.is_empty() {
        println!(
            "      ahead branches   {}",
            ahead_branches.join(", ").yellow()
        );
    }
    println!("      uncommitted      {}", yes_no(a.dirty, true));
    println!(
        "      stashes          {}",
        count_colour(a.stash_count, true)
    );
    println!(
        "      tags             {}",
        a.tag_count.to_string().dimmed()
    );
    println!(
        "      untracked        {} (ignored {})",
        count_colour(a.untracked, true),
        a.ignored.to_string().dimmed()
    );
    println!(
        "      worktrees        {}",
        a.worktree_count.to_string().dimmed()
    );
    if a.submodule_count > 0 {
        println!(
            "      submodules       {}",
            a.submodule_count.to_string().yellow().bold()
        );
    }
    println!("      size             {}", format_size(a.size).bold());
}

fn short_sha(sha: &Option<String>) -> String {
    sha.as_deref()
        .map(|s| s.chars().take(10).collect())
        .unwrap_or_else(|| "unknown".to_string())
}

fn yes_no(flag: bool, warn_when_true: bool) -> colored::ColoredString {
    if flag {
        if warn_when_true {
            "yes".red().bold()
        } else {
            "yes".green().bold()
        }
    } else if warn_when_true {
        "no".green()
    } else {
        "no".red()
    }
}

fn count_colour(n: u64, warn_when_nonzero: bool) -> colored::ColoredString {
    if n == 0 {
        "0".green()
    } else if warn_when_nonzero {
        n.to_string().red().bold()
    } else {
        n.to_string().yellow()
    }
}
