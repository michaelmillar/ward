use anyhow::Result;
use colored::Colorize;
use std::path::PathBuf;
use walkdir::WalkDir;

use crate::scan::{dir_size, find_git_repos, format_size, last_commit_date};

const ARTIFACT_DIRS: &[&str] = &[
    "target",
    "node_modules",
    ".next",
    "dist",
    "__pycache__",
    ".gradle",
    "build",
];

pub fn run(path: Option<PathBuf>) -> Result<()> {
    let root = path.unwrap_or_else(crate::scan::default_projects_path);
    if !root.exists() {
        anyhow::bail!("Path does not exist: {}", root.display());
    }

    println!(
        "{}",
        format!("Analysing {} ...", root.display()).dimmed()
    );

    let total_size = dir_size(&root);
    let repos = find_git_repos(&root);
    let repo_count = repos.len();

    let mut artifact_size = 0u64;
    let mut skip_prefixes: Vec<PathBuf> = Vec::new();
    let walker = WalkDir::new(&root).into_iter();
    for entry in walker.filter_entry(|e| {
        if !e.file_type().is_dir() {
            return true;
        }
        let name = e.file_name().to_string_lossy();
        if name.starts_with('.') && e.depth() > 0 && name != ".next" && name != ".gradle" {
            return false;
        }
        true
    }) {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_dir() {
            continue;
        }
        if skip_prefixes.iter().any(|p| entry.path().starts_with(p)) {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if ARTIFACT_DIRS.contains(&name.as_ref()) {
            artifact_size += dir_size(entry.path());
            skip_prefixes.push(entry.path().to_path_buf());
        }
    }

    let source_size = total_size.saturating_sub(artifact_size);

    println!();
    println!("{}", "Disk Usage Overview".bold().underline());
    println!("  Total:              {}", format_size(total_size).bold());
    println!(
        "  Build artefacts:    {} ({}%)",
        format_size(artifact_size).red(),
        if total_size > 0 {
            artifact_size * 100 / total_size
        } else {
            0
        }
    );
    println!(
        "  Source and other:    {} ({}%)",
        format_size(source_size).green(),
        if total_size > 0 {
            source_size * 100 / total_size
        } else {
            0
        }
    );
    println!("  Git repositories:   {}", repo_count);

    if !repos.is_empty() {
        let mut stale: Vec<(PathBuf, chrono::NaiveDate)> = repos
            .iter()
            .filter_map(|r| last_commit_date(r).map(|d| (r.clone(), d)))
            .collect();
        stale.sort_by(|a, b| a.1.cmp(&b.1));

        let show_count = stale.len().min(5);
        if show_count > 0 {
            println!();
            println!("{}", "Stalest Repositories".bold().underline());
            for (path, date) in stale.iter().take(show_count) {
                let size = dir_size(path);
                println!(
                    "  {} ({}, last commit: {})",
                    path.display(),
                    format_size(size),
                    date.to_string().yellow()
                );
            }
        }
    }

    Ok(())
}
