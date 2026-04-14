use anyhow::{bail, Result};
use colored::Colorize;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::{Config, ExcludeOpts, Exclusions};
use crate::git::{self, normalise_remote_url};
use crate::util::{default_projects_path, dir_size, format_size};

struct CloneInfo {
    path: PathBuf,
    size: u64,
    last_commit: Option<chrono::NaiveDate>,
    root_sha: Option<String>,
    has_local_only: bool,
    has_stash: bool,
    dirty: bool,
}

pub fn run(path: Option<PathBuf>, convert: bool, exclude_opts: ExcludeOpts) -> Result<()> {
    let cfg = Config::load();
    let root = path.unwrap_or_else(default_projects_path);
    if !root.exists() {
        bail!("Path does not exist: {}", root.display());
    }

    println!(
        "{}",
        format!("Scanning {} for duplicate clones ...", root.display()).dimmed()
    );

    let exclusions = Exclusions::build(&cfg, &exclude_opts);
    let repos: Vec<_> = git::find_git_repos(&root)
        .into_iter()
        .filter(|r| !exclusions.is_excluded(r))
        .collect();
    let enriched: Vec<(String, String, CloneInfo)> = repos
        .par_iter()
        .filter_map(|r| {
            let url = git::origin_url(r)?;
            let key = normalise_remote_url(&url);
            let root_sha = git::root_commit_sha(r);
            let info = CloneInfo {
                path: r.clone(),
                size: dir_size(r),
                last_commit: git::last_commit_date(r),
                root_sha: root_sha.clone(),
                has_local_only: git::branches(r).iter().any(|b| b.upstream.is_none()),
                has_stash: git::stash_count(r) > 0,
                dirty: git::has_uncommitted_changes(r).unwrap_or(true),
            };
            let fingerprint = match root_sha {
                Some(sha) => format!("{key}#{}", sha.chars().take(12).collect::<String>()),
                None => key,
            };
            Some((fingerprint, url, info))
        })
        .collect();

    let mut by_fp: HashMap<String, (String, Vec<CloneInfo>)> = HashMap::new();
    for (fp, url, info) in enriched {
        by_fp
            .entry(fp)
            .or_insert_with(|| (url, Vec::new()))
            .1
            .push(info);
    }

    let mut clusters: Vec<(String, String, Vec<CloneInfo>)> = by_fp
        .into_iter()
        .filter(|(_, (_, v))| v.len() > 1)
        .map(|(fp, (url, v))| (fp, url, v))
        .collect();

    clusters.sort_by(|a, b| {
        let sa: u64 = a.2.iter().map(|c| c.size).sum();
        let sb: u64 = b.2.iter().map(|c| c.size).sum();
        sb.cmp(&sa)
    });

    if clusters.is_empty() {
        println!("{}", "No duplicate clones found.".green());
        return Ok(());
    }

    println!(
        "{}",
        format!("Found {} duplicate cluster(s)", clusters.len()).yellow()
    );
    println!();

    let mut total_reclaimable = 0u64;
    let mut plans = Vec::new();

    for (_fp, url, mut clones) in clusters {
        clones.sort_by(|a, b| b.last_commit.cmp(&a.last_commit));
        let cluster_total: u64 = clones.iter().map(|c| c.size).sum();
        let keeper = &clones[0];
        let others = &clones[1..];
        let reclaimable: u64 = others.iter().map(|c| c.size).sum();
        total_reclaimable += reclaimable;

        println!("  {}", url.bold());
        println!(
            "    cluster total {}, reclaimable {}",
            format_size(cluster_total),
            format_size(reclaimable).green().bold()
        );

        let keep_date = keeper
            .last_commit
            .map(|d| d.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        println!(
            "    [{}]   {} ({}, last commit {})",
            "keep".green().bold(),
            keeper.path.display(),
            format_size(keeper.size),
            keep_date.dimmed()
        );

        for c in others {
            let action = decide_action(c);
            let date = c
                .last_commit
                .map(|d| d.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let label = match action {
                Action::Worktree => "worktree".blue().bold(),
                Action::Remove => "remove".red().bold(),
                Action::Skip => "skip".yellow().bold(),
            };
            println!(
                "    [{}] {} ({}, last commit {})",
                label,
                c.path.display(),
                format_size(c.size),
                date.dimmed()
            );
            print_action_rationale(c, action);
            plans.push((keeper.path.clone(), c.path.clone(), action));
        }
        println!();
    }

    println!(
        "{} total reclaimable across clusters",
        format_size(total_reclaimable).bold()
    );

    if !convert {
        println!();
        println!(
            "{}",
            "Dry run. Use --convert to execute the worktree and removal plan.".yellow()
        );
        return Ok(());
    }

    println!();
    println!("{}", "Executing plan ...".bold());
    let mut converted = 0u64;
    let mut removed = 0u64;
    let mut skipped = 0u64;
    for (keeper, target, action) in plans {
        match action {
            Action::Worktree => match convert_to_worktree(&keeper, &target) {
                Ok(()) => {
                    converted += 1;
                    println!(
                        "  {} converted {} to worktree of {}",
                        "ok".green().bold(),
                        target.display(),
                        keeper.display()
                    );
                }
                Err(e) => {
                    skipped += 1;
                    eprintln!(
                        "  {} failed to convert {} ({})",
                        "error".red().bold(),
                        target.display(),
                        e
                    );
                }
            },
            Action::Remove => match std::fs::remove_dir_all(&target) {
                Ok(()) => {
                    removed += 1;
                    println!("  {} removed {}", "ok".green().bold(), target.display());
                }
                Err(e) => {
                    skipped += 1;
                    eprintln!(
                        "  {} failed to remove {} ({})",
                        "error".red().bold(),
                        target.display(),
                        e
                    );
                }
            },
            Action::Skip => {
                skipped += 1;
                println!("  {} skipped {}", "skip".yellow().bold(), target.display());
            }
        }
    }

    println!();
    println!(
        "Converted {}, removed {}, skipped {}",
        converted.to_string().green().bold(),
        removed.to_string().red().bold(),
        skipped.to_string().yellow().bold()
    );
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Worktree,
    Remove,
    Skip,
}

fn decide_action(c: &CloneInfo) -> Action {
    if c.dirty || c.has_stash {
        return Action::Skip;
    }
    if c.has_local_only {
        return Action::Worktree;
    }
    Action::Remove
}

fn print_action_rationale(c: &CloneInfo, action: Action) {
    let reasons = match action {
        Action::Worktree => "has local-only branches, preserve via worktree",
        Action::Remove => "no local work, safe to remove",
        Action::Skip => "uncommitted work or stashes present",
    };
    println!("          {} {}", "reason".dimmed(), reasons.dimmed());
    if c.root_sha.is_none() {
        println!(
            "          {} {}",
            "warn".yellow().bold().dimmed(),
            "no root commit found, fingerprint may be weak"
                .yellow()
                .dimmed()
        );
    }
}

fn convert_to_worktree(keeper: &std::path::Path, target: &std::path::Path) -> Result<()> {
    let branch_out = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(target)
        .output()?;
    let branch = String::from_utf8_lossy(&branch_out.stdout)
        .trim()
        .to_string();
    if branch.is_empty() {
        bail!("target has detached HEAD, refusing to convert");
    }

    let push_out = std::process::Command::new("git")
        .args(["push", "--set-upstream", "origin", &branch])
        .current_dir(target)
        .output();
    if let Ok(o) = &push_out {
        if !o.status.success() {
            println!(
                "          {} push failed, branch preserved in keeper via fetch",
                "note".yellow().bold().dimmed()
            );
        }
    }

    let fetch_out = std::process::Command::new("git")
        .args(["fetch", "origin", &branch])
        .current_dir(keeper)
        .output()?;
    if !fetch_out.status.success() {
        let err = String::from_utf8_lossy(&fetch_out.stderr).to_string();
        bail!("fetch failed in keeper: {err}");
    }

    std::fs::remove_dir_all(target)?;

    let add_out = std::process::Command::new("git")
        .args(["worktree", "add"])
        .arg(target)
        .arg(&branch)
        .current_dir(keeper)
        .output()?;
    if !add_out.status.success() {
        let err = String::from_utf8_lossy(&add_out.stderr).to_string();
        bail!("worktree add failed: {err}");
    }
    Ok(())
}
