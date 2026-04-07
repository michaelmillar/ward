use anyhow::{Context, Result, bail};
use chrono::Utc;
use colored::Colorize;
use flate2::Compression;
use flate2::write::GzEncoder;
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};

use crate::assess::{Assessment, Verdict, assess_repo, print_safety_proof};
use crate::bundle;
use crate::cache::Cache;
use crate::config::Config;
use crate::git;
use crate::manifest::{self, ArchiveFormat, MANIFEST_VERSION, Manifest, RefEntry, RemoteEntry};
use crate::util::{archives_dir, default_projects_path, format_size};

const VERIFIER_VERSION: &str = concat!("ward ", env!("CARGO_PKG_VERSION"));

pub fn run(
    path: Option<PathBuf>,
    execute: bool,
    include_prototypes: bool,
    include_no_remote: bool,
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
        println!(
            "{}",
            format!("Assessing git repos under {} ...", root.display()).dimmed()
        );
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

    assessments.sort_by(|a, b| a.last_commit.cmp(&b.last_commit));

    let eligible: Vec<&Assessment> = assessments
        .iter()
        .filter(|a| match a.verdict {
            Verdict::Archive => true,
            Verdict::Prototype => include_prototypes,
            Verdict::NoRemote => include_no_remote,
            _ => false,
        })
        .collect();

    if as_json {
        let payload: Vec<&Assessment> = eligible.to_vec();
        let json = serde_json::to_string_pretty(&payload)?;
        println!("{json}");
        return Ok(());
    }

    print_summary(&assessments, &eligible);

    if eligible.is_empty() {
        println!("{}", "Nothing to archive.".yellow());
        return Ok(());
    }

    for a in &eligible {
        print_candidate(a);
    }

    let total_size: u64 = eligible.iter().map(|a| a.size).sum();
    println!();
    println!(
        "{} repo(s) eligible for archive ({})",
        eligible.len(),
        format_size(total_size).bold()
    );

    if !execute {
        println!();
        println!(
            "{}",
            "Dry run. Use --execute to archive these repos.".yellow()
        );
        return Ok(());
    }

    let archive_dir = archives_dir();
    fs::create_dir_all(&archive_dir)?;

    let mut freed = 0u64;
    let mut failures = 0u64;
    for a in &eligible {
        match archive_one(a, &archive_dir) {
            Ok(()) => freed += a.size,
            Err(e) => {
                failures += 1;
                eprintln!(
                    "  {} Failed to archive {} ({})",
                    "error".red().bold(),
                    a.path.display(),
                    e
                );
            }
        }
    }

    println!();
    println!(
        "Archived {} repo(s), {} reclaimed",
        eligible.len() - failures as usize,
        format_size(freed).green().bold()
    );
    if failures > 0 {
        println!("{} failure(s)", failures.to_string().red().bold());
    }

    Ok(())
}

fn print_summary(all: &[Assessment], eligible: &[&Assessment]) {
    let mut counts = std::collections::BTreeMap::new();
    for a in all {
        *counts.entry(verdict_key(a.verdict)).or_insert(0u64) += 1;
    }
    println!();
    println!("{}", "Verdict summary".bold().underline());
    for (k, v) in &counts {
        println!("  {:<12} {}", k, v);
    }
    println!("  {:<12} {}", "eligible", eligible.len());
    println!();
}

fn verdict_key(v: Verdict) -> &'static str {
    match v {
        Verdict::Archive => "archive",
        Verdict::Prototype => "prototype",
        Verdict::Worktree => "worktree",
        Verdict::HasLocalWork => "local-work",
        Verdict::KeepAsIs => "keep",
        Verdict::NoRemote => "no-remote",
    }
}

fn print_candidate(a: &Assessment) {
    let date_str = a
        .last_commit
        .map(|d| d.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    println!(
        "  [{}] {} ({}, last commit {})",
        a.verdict.label(),
        a.path.display(),
        format_size(a.size),
        date_str.dimmed()
    );
    print_safety_proof(a);
    println!();
}

fn archive_one(a: &Assessment, archive_dir: &Path) -> Result<()> {
    let repo_name = a
        .path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "repo".to_string());
    let stamp = Utc::now().format("%Y%m%d%H%M%S").to_string();
    let stem = format!("{repo_name}-{stamp}");
    let bundle_path = archive_dir.join(format!("{stem}.bundle"));
    let extras_path = archive_dir.join(format!("{stem}.extras.tar.gz"));
    let manifest_path = archive_dir.join(format!("{stem}.json"));

    let stash_shas = if a.stash_count > 0 {
        println!("    preserving {} stash(es) as temp refs ...", a.stash_count);
        git::create_stash_refs(&a.path).unwrap_or_default()
    } else {
        Vec::new()
    };

    println!("  Bundling {} ...", a.path.display());
    let bundle_result = bundle::create(&a.path, &bundle_path);
    git::cleanup_stash_refs(&a.path);
    bundle_result.with_context(|| "git bundle create")?;

    bundle::verify(&bundle_path).with_context(|| "git bundle verify")?;
    let bundle_sha = bundle::sha256_file(&bundle_path)?;

    let hooks = git::custom_hooks(&a.path);
    let has_config = a.path.join(".git/config").exists();
    let has_extras = a.untracked > 0 || !hooks.is_empty() || has_config;

    let extras_sha = if has_extras {
        let mut parts = Vec::new();
        if a.untracked > 0 {
            parts.push(format!("{} untracked", a.untracked));
        }
        if !hooks.is_empty() {
            parts.push(format!("{} hook(s)", hooks.len()));
        }
        if has_config {
            parts.push("config".to_string());
        }
        println!("    capturing extras [{}]", parts.join(", "));
        create_extras_tar(&a.path, &extras_path, &hooks, has_config)?;
        Some(bundle::sha256_file(&extras_path)?)
    } else {
        None
    };

    println!("    verifying by clone ...");
    let verified = bundle::verify_by_clone(&bundle_path)?;
    compare_refs(a, &verified)?;

    let stash_refs_in_bundle = verified
        .refs
        .iter()
        .filter(|(n, _)| n.starts_with("refs/ward-stash/"))
        .count() as u64;

    let refs: Vec<RefEntry> = verified
        .refs
        .iter()
        .map(|(n, s)| RefEntry {
            name: n.clone(),
            sha: s.clone(),
        })
        .collect();

    let remotes: Vec<RemoteEntry> = a
        .remotes
        .iter()
        .map(|(n, u)| RemoteEntry {
            name: n.clone(),
            url: u.clone(),
        })
        .collect();

    let manifest = Manifest {
        manifest_version: MANIFEST_VERSION,
        format: ArchiveFormat::Bundle,
        original_path: a.path.display().to_string(),
        archived_at: Utc::now().to_rfc3339(),
        archived_by: VERIFIER_VERSION.to_string(),
        size_bytes: a.size,
        bundle_sha256: Some(bundle_sha),
        extras_sha256: extras_sha,
        head_sha: a.head_sha.clone(),
        refs,
        remotes,
        first_commit: a.first_commit.map(|d| d.to_string()),
        last_commit: a.last_commit.map(|d| d.to_string()),
        commit_count: a.commit_count,
        verified_at: Some(Utc::now().to_rfc3339()),
        verifier_version: Some(VERIFIER_VERSION.to_string()),
        has_extras,
        stash_count: stash_refs_in_bundle,
        stash_shas,
        tag_count: a.tag_count,
        has_hooks: !hooks.is_empty(),
        has_config,
        submodule_count: a.submodule_count,
    };

    manifest::write(&manifest, &manifest_path)?;

    fs::remove_dir_all(&a.path)
        .with_context(|| format!("Failed to remove {}", a.path.display()))?;

    println!(
        "    {} archived {} ({})",
        "ok".green().bold(),
        bundle_path.display(),
        format_size(fs::metadata(&bundle_path)?.len())
    );
    Ok(())
}

fn compare_refs(a: &Assessment, verified: &bundle::VerifiedRefs) -> Result<()> {
    if let Some(head) = &a.head_sha {
        if let Some(vhead) = &verified.head {
            if head != vhead {
                bail!("HEAD mismatch after bundle verification");
            }
        }
    }
    if verified.refs.is_empty() {
        bail!("Verification clone contains no refs");
    }
    Ok(())
}

fn create_extras_tar(
    repo: &Path,
    dest: &Path,
    hooks: &[PathBuf],
    include_config: bool,
) -> Result<()> {
    let file = fs::File::create(dest)?;
    let encoder = GzEncoder::new(file, Compression::default());
    let mut archive = tar::Builder::new(encoder);

    let output = std::process::Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard"])
        .current_dir(repo)
        .output()?;
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    for line in text.lines() {
        let rel = line.trim();
        if rel.is_empty() {
            continue;
        }
        let full = repo.join(rel);
        if full.is_file() {
            let mut f = fs::File::open(&full)?;
            archive.append_file(rel, &mut f)?;
        }
    }

    if include_config {
        let config_path = repo.join(".git/config");
        if config_path.is_file() {
            let mut f = fs::File::open(&config_path)?;
            archive.append_file(".ward-extras/config", &mut f)?;
        }
    }

    for hook in hooks {
        if let Some(name) = hook.file_name() {
            let mut f = fs::File::open(hook)?;
            let tar_path = format!(".ward-extras/hooks/{}", name.to_string_lossy());
            archive.append_file(tar_path, &mut f)?;
        }
    }

    archive.finish()?;
    Ok(())
}

