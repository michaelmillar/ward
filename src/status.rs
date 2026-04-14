use anyhow::{bail, Result};
use colored::Colorize;
use rayon::prelude::*;
use std::path::PathBuf;
use walkdir::WalkDir;

use crate::assess::{assess_repo, Assessment, Verdict};
use crate::cache::Cache;
use crate::config::{Config, ExcludeOpts, Exclusions};
use crate::git;
use crate::util::{default_projects_path, dir_size, format_size};

const ARTIFACT_DIRS: &[&str] = &[
    "target",
    "node_modules",
    ".next",
    "dist",
    "__pycache__",
    ".gradle",
    "build",
    ".turbo",
    ".vite",
    ".parcel-cache",
    ".swc",
    ".pnpm-store",
    ".venv",
    ".export-venv",
    "venv",
    ".ruff_cache",
    ".mypy_cache",
    ".pytest_cache",
    ".tox",
    ".nox",
    "zig-cache",
    "zig-out",
    ".dart_tool",
    "DerivedData",
];

pub fn run(
    path: Option<PathBuf>,
    no_cache: bool,
    as_json: bool,
    exclude_opts: ExcludeOpts,
) -> Result<()> {
    let cfg = Config::load();
    let exclusions = Exclusions::build(&cfg, &exclude_opts);
    let root = path
        .or_else(|| cfg.workspace_root())
        .unwrap_or_else(default_projects_path);
    if !root.exists() {
        bail!("Path does not exist: {}", root.display());
    }

    if !as_json {
        println!("{}", format!("Analysing {} ...", root.display()).dimmed());
    }

    let total_size = dir_size(&root);
    let repos = git::find_git_repos(&root);
    let repo_count = repos.len();

    let mut artifact_size = 0u64;
    let mut it = WalkDir::new(&root).into_iter();
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
        if ARTIFACT_DIRS.contains(&name.as_str()) {
            artifact_size += dir_size(entry.path());
            it.skip_current_dir();
            continue;
        }
        if name.starts_with('.') && entry.depth() > 0 {
            it.skip_current_dir();
        }
    }

    let source_size = total_size.saturating_sub(artifact_size);

    if !as_json {
        println!();
        println!("{}", "Disk Usage".bold().underline());
        println!("  Total              {}", format_size(total_size).bold());
        println!(
            "  Build artefacts    {} ({}%)",
            format_size(artifact_size).red(),
            if total_size > 0 {
                artifact_size * 100 / total_size
            } else {
                0
            }
        );
        println!(
            "  Source and other   {} ({}%)",
            format_size(source_size).green(),
            if total_size > 0 {
                source_size * 100 / total_size
            } else {
                0
            }
        );
        println!("  Git repositories   {}", repo_count);
    }

    if repos.is_empty() {
        if as_json {
            println!(
                "{}",
                serde_json::json!({
                    "total_size": total_size,
                    "artifact_size": artifact_size,
                    "source_size": source_size,
                    "repo_count": repo_count,
                    "assessments": []
                })
            );
        }
        return Ok(());
    }

    let mut cache = if no_cache {
        Cache::default()
    } else {
        Cache::load()
    };
    let thresholds = cfg.thresholds.clone();
    let assessments: Vec<Assessment> = repos
        .par_iter()
        .filter(|r| !exclusions.is_excluded(r))
        .filter_map(|r| {
            if !no_cache {
                if let Some(a) = cache.lookup(r) {
                    return Some(a.clone());
                }
            }
            assess_repo(r, &thresholds).ok()
        })
        .collect();

    if !no_cache {
        for a in &assessments {
            cache.store(&a.path, a.clone());
        }
        let _ = cache.save();
    }

    if as_json {
        #[derive(serde::Serialize)]
        struct StatusReport<'a> {
            total_size: u64,
            artifact_size: u64,
            source_size: u64,
            repo_count: usize,
            assessments: &'a [Assessment],
        }
        let report = StatusReport {
            total_size,
            artifact_size,
            source_size,
            repo_count,
            assessments: &assessments,
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    let mut counts = std::collections::BTreeMap::new();
    let mut sizes = std::collections::BTreeMap::new();
    for a in &assessments {
        let k = match a.verdict {
            Verdict::Archive => "archive",
            Verdict::Prototype => "prototype",
            Verdict::Worktree => "worktree",
            Verdict::HasLocalWork => "local-work",
            Verdict::KeepAsIs => "keep",
            Verdict::NoRemote => "no-remote",
        };
        *counts.entry(k).or_insert(0u64) += 1;
        *sizes.entry(k).or_insert(0u64) += a.size;
    }

    println!();
    println!("{}", "Lifecycle".bold().underline());
    for (k, c) in &counts {
        let s = sizes.get(k).copied().unwrap_or(0);
        println!("  {:<12} {:>4}  {}", k, c, format_size(s).dimmed());
    }

    let mut stale: Vec<(PathBuf, chrono::NaiveDate, u64)> = assessments
        .iter()
        .filter_map(|a| a.last_commit.map(|d| (a.path.clone(), d, a.size)))
        .collect();
    stale.sort_by(|a, b| a.1.cmp(&b.1));

    let show_count = stale.len().min(5);
    if show_count > 0 {
        println!();
        println!("{}", "Stalest repositories".bold().underline());
        for (path, date, size) in stale.iter().take(show_count) {
            println!(
                "  {} ({}, last commit {})",
                path.display(),
                format_size(*size),
                date.to_string().yellow()
            );
        }
    }

    if artifact_size > 0 {
        println!();
        println!(
            "  Run {} to reclaim {} of build artefacts",
            "ward clean".bold(),
            format_size(artifact_size).red()
        );
    }

    let archivable = assessments
        .iter()
        .filter(|a| a.verdict == Verdict::Archive || a.verdict == Verdict::Prototype)
        .count();
    if archivable > 0 {
        println!(
            "  {} repo(s) ready for {}",
            archivable,
            "ward archive".bold()
        );
    }

    Ok(())
}
