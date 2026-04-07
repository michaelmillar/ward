use anyhow::{Result, bail};
use colored::Colorize;
use rayon::prelude::*;
use std::path::PathBuf;

use crate::assess::{Assessment, Verdict, assess_repo};
use crate::cache::Cache;
use crate::config::Config;
use crate::git;
use crate::util::{default_projects_path, format_size};

pub fn run(
    path: Option<PathBuf>,
    prototypes_only: bool,
    verdict_filter: Option<String>,
    no_cache: bool,
    as_json: bool,
) -> Result<()> {
    let cfg = Config::load();
    let root = path
        .or_else(|| cfg.workspace_root())
        .unwrap_or_else(default_projects_path);
    if !root.exists() {
        bail!("Path does not exist: {}", root.display());
    }

    if !as_json {
        println!("{}", format!("Scanning {} ...", root.display()).dimmed());
    }

    let repos: Vec<_> = git::find_git_repos(&root)
        .into_iter()
        .filter(|r| !cfg.is_excluded(r))
        .collect();
    if repos.is_empty() {
        if !as_json {
            println!("{}", "No git repos found.".yellow());
        } else {
            println!("[]");
        }
        return Ok(());
    }

    let mut cache = if no_cache { Cache::default() } else { Cache::load() };
    let thresholds = cfg.thresholds.clone();

    let mut assessments: Vec<Assessment> = repos
        .par_iter()
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

    let filter_verdict = verdict_filter
        .as_deref()
        .and_then(parse_verdict);

    if let Some(v) = filter_verdict {
        assessments.retain(|a| a.verdict == v);
    }
    if prototypes_only {
        assessments.retain(|a| a.verdict == Verdict::Prototype);
    }

    assessments.sort_by(|a, b| b.size.cmp(&a.size));

    if as_json {
        let json = serde_json::to_string_pretty(&assessments)?;
        println!("{json}");
        return Ok(());
    }

    if assessments.is_empty() {
        println!("{}", "No repos match the filter.".yellow());
        return Ok(());
    }

    print_verdict_histogram(&assessments);

    println!();
    println!("{}", "Per-repo verdicts".bold().underline());
    for a in &assessments {
        print_row(a);
    }

    println!();
    println!("{}", "Next actions".bold().underline());
    print_next_actions(&assessments);

    Ok(())
}

fn parse_verdict(s: &str) -> Option<Verdict> {
    match s.to_lowercase().as_str() {
        "archive" => Some(Verdict::Archive),
        "prototype" => Some(Verdict::Prototype),
        "worktree" => Some(Verdict::Worktree),
        "local" | "local-work" => Some(Verdict::HasLocalWork),
        "keep" => Some(Verdict::KeepAsIs),
        "no-remote" | "noremote" => Some(Verdict::NoRemote),
        _ => None,
    }
}

fn print_verdict_histogram(assessments: &[Assessment]) {
    let mut counts = std::collections::BTreeMap::new();
    let mut sizes = std::collections::BTreeMap::new();
    for a in assessments {
        let key = match a.verdict {
            Verdict::Archive => "archive",
            Verdict::Prototype => "prototype",
            Verdict::Worktree => "worktree",
            Verdict::HasLocalWork => "local-work",
            Verdict::KeepAsIs => "keep",
            Verdict::NoRemote => "no-remote",
        };
        *counts.entry(key).or_insert(0u64) += 1;
        *sizes.entry(key).or_insert(0u64) += a.size;
    }
    println!();
    println!("{}", "Verdict summary".bold().underline());
    for (k, c) in &counts {
        let s = sizes.get(k).copied().unwrap_or(0);
        println!("  {:<12} {:>4}  {}", k, c, format_size(s).dimmed());
    }
}

fn print_row(a: &Assessment) {
    let date_str = a
        .last_commit
        .map(|d| d.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    println!(
        "  [{}] {} ({}, last commit {}, {} commits)",
        a.verdict.label(),
        a.path.display(),
        format_size(a.size),
        date_str.dimmed(),
        a.commit_count
    );
}

fn print_next_actions(assessments: &[Assessment]) {
    let archive_count = assessments.iter().filter(|a| a.verdict == Verdict::Archive).count();
    let proto_count = assessments.iter().filter(|a| a.verdict == Verdict::Prototype).count();
    let noremote_count = assessments.iter().filter(|a| a.verdict == Verdict::NoRemote).count();

    if archive_count > 0 {
        println!(
            "  {} stale repos ready for bundle archive, run {}",
            archive_count,
            "ward archive --execute".bold()
        );
    }
    if proto_count > 0 {
        println!(
            "  {} prototype repos detected, run {}",
            proto_count,
            "ward archive --prototypes --execute".bold()
        );
    }
    if noremote_count > 0 {
        println!(
            "  {} repos without a remote, run {} to see them",
            noremote_count,
            "ward scan --verdict no-remote".bold()
        );
    }
    if archive_count == 0 && proto_count == 0 {
        println!("  {}", "Workspace is clean. Nothing to reclaim.".green());
    }
}
