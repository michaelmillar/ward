use anyhow::Result;
use colored::Colorize;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::scan::{dir_size, find_git_repos, format_size, git_remote_url, last_commit_date};

struct RepoInfo {
    path: PathBuf,
    size: u64,
    last_commit: Option<chrono::NaiveDate>,
}

pub fn run(path: Option<PathBuf>) -> Result<()> {
    let root = path.unwrap_or_else(crate::scan::default_projects_path);
    if !root.exists() {
        anyhow::bail!("Path does not exist: {}", root.display());
    }

    println!(
        "{}",
        format!("Scanning {} for duplicate git repos...", root.display()).dimmed()
    );

    let repos = find_git_repos(&root);
    let mut by_remote: HashMap<String, Vec<RepoInfo>> = HashMap::new();

    for repo in repos {
        if let Some(url) = git_remote_url(&repo) {
            let normalised = normalise_url(&url);
            let size = dir_size(&repo);
            let last_commit = last_commit_date(&repo);
            by_remote.entry(normalised).or_default().push(RepoInfo {
                path: repo,
                size,
                last_commit,
            });
        }
    }

    let mut duplicates: Vec<(String, Vec<RepoInfo>)> = by_remote
        .into_iter()
        .filter(|(_, repos)| repos.len() > 1)
        .collect();

    duplicates.sort_by(|a, b| {
        let total_a: u64 = a.1.iter().map(|r| r.size).sum();
        let total_b: u64 = b.1.iter().map(|r| r.size).sum();
        total_b.cmp(&total_a)
    });

    if duplicates.is_empty() {
        println!("{}", "No duplicate repos found.".green());
        return Ok(());
    }

    println!(
        "{}",
        format!("Found {} duplicate group(s):", duplicates.len()).yellow()
    );
    println!();

    for (url, mut repos) in duplicates {
        println!("  {}", url.bold());

        repos.sort_by(|a, b| b.last_commit.cmp(&a.last_commit));

        for (i, repo) in repos.iter().enumerate() {
            let date_str = repo
                .last_commit
                .map(|d| d.to_string())
                .unwrap_or_else(|| "unknown".to_string());

            let label = if i == 0 {
                "keep".green().bold()
            } else {
                "remove".red().bold()
            };

            println!(
                "    [{}] {} ({}, last commit: {})",
                label,
                repo.path.display(),
                format_size(repo.size),
                date_str.dimmed()
            );
        }
        println!();
    }

    Ok(())
}

fn normalise_url(url: &str) -> String {
    let s = url.trim();
    let s = s.strip_suffix(".git").unwrap_or(s);
    let s = if let Some(rest) = s.strip_prefix("git@") {
        rest.replacen(':', "/", 1)
    } else {
        s.to_string()
    };
    let s = s.strip_prefix("https://").unwrap_or(&s);
    let s = s.strip_prefix("http://").unwrap_or(s);
    s.to_lowercase()
}
