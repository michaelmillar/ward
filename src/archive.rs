use anyhow::{Context, Result};
use chrono::Utc;
use colored::Colorize;
use flate2::write::GzEncoder;
use flate2::read::GzDecoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::scan::{
    all_branches_pushed, dir_size, find_git_repos, format_size, has_uncommitted_changes,
    last_commit_date, reap_archives_dir,
};

#[derive(Serialize, Deserialize)]
struct ArchiveManifest {
    original_path: String,
    archived_at: String,
    last_commit: Option<String>,
    size_bytes: u64,
}

struct RepoAssessment {
    path: PathBuf,
    size: u64,
    last_commit: Option<chrono::NaiveDate>,
    all_pushed: bool,
    dirty: bool,
    pm_status: Option<String>,
    safe_to_archive: bool,
}

fn check_pm_status(repo_path: &Path) -> Option<String> {
    let db_path = crate::scan::dirs_home().join(".local/share/pm/projects.db");
    if !db_path.exists() {
        return None;
    }
    let conn = rusqlite::Connection::open(&db_path).ok()?;
    let repo_name = repo_path.file_name()?.to_string_lossy().to_string();
    conn.query_row(
        "SELECT status FROM projects WHERE name = ?1",
        [&repo_name],
        |row| row.get::<_, String>(0),
    )
    .ok()
}

pub fn run(path: Option<PathBuf>, execute: bool) -> Result<()> {
    let root = path.unwrap_or_else(crate::scan::default_projects_path);
    if !root.exists() {
        anyhow::bail!("Path does not exist: {}", root.display());
    }

    println!(
        "{}",
        format!("Scanning {} for archivable repos...", root.display()).dimmed()
    );

    let repos = find_git_repos(&root);
    let mut assessments = Vec::new();

    for repo in repos {
        let size = dir_size(&repo);
        let last_commit = last_commit_date(&repo);
        let all_pushed = all_branches_pushed(&repo).unwrap_or(false);
        let dirty = has_uncommitted_changes(&repo).unwrap_or(true);
        let pm_status = check_pm_status(&repo);

        let safe = all_pushed && !dirty;

        assessments.push(RepoAssessment {
            path: repo,
            size,
            last_commit,
            all_pushed,
            dirty,
            pm_status,
            safe_to_archive: safe,
        });
    }

    assessments.sort_by(|a, b| a.last_commit.cmp(&b.last_commit));

    if assessments.is_empty() {
        println!("{}", "No git repos found.".yellow());
        return Ok(());
    }

    for assessment in &assessments {
        let date_str = assessment
            .last_commit
            .map(|d| d.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let status_indicator = if assessment.safe_to_archive {
            "SAFE".green().bold()
        } else {
            "SKIP".red().bold()
        };

        println!(
            "  [{}] {} ({})",
            status_indicator,
            assessment.path.display(),
            format_size(assessment.size),
        );

        println!(
            "        Last commit: {} | Pushed: {} | Clean: {}",
            date_str.dimmed(),
            if assessment.all_pushed {
                "yes".green()
            } else {
                "no".red()
            },
            if !assessment.dirty {
                "yes".green()
            } else {
                "no".red()
            },
        );

        if let Some(ref status) = assessment.pm_status {
            println!("        PM status: {}", status.cyan());
        }
    }

    let safe_count = assessments.iter().filter(|a| a.safe_to_archive).count();
    let safe_size: u64 = assessments
        .iter()
        .filter(|a| a.safe_to_archive)
        .map(|a| a.size)
        .sum();

    println!();
    println!(
        "{} repo(s) safe to archive ({})",
        safe_count,
        format_size(safe_size).bold()
    );

    if execute {
        let archive_dir = reap_archives_dir();
        fs::create_dir_all(&archive_dir)?;

        for assessment in assessments.iter().filter(|a| a.safe_to_archive) {
            let repo_name = assessment
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());

            let archive_name = format!(
                "{}-{}.tar.gz",
                repo_name,
                Utc::now().format("%Y%m%d%H%M%S")
            );
            let archive_path = archive_dir.join(&archive_name);
            let manifest_path = archive_dir.join(format!("{archive_name}.json"));

            println!("  Archiving {} ...", assessment.path.display());

            match create_archive(&assessment.path, &archive_path) {
                Ok(()) => {
                    let manifest = ArchiveManifest {
                        original_path: assessment.path.display().to_string(),
                        archived_at: Utc::now().to_rfc3339(),
                        last_commit: assessment.last_commit.map(|d| d.to_string()),
                        size_bytes: assessment.size,
                    };
                    let json = serde_json::to_string_pretty(&manifest)?;
                    fs::write(&manifest_path, json)?;

                    fs::remove_dir_all(&assessment.path)?;
                    println!(
                        "    {} Archived to {}",
                        "Done".green(),
                        archive_path.display()
                    );
                }
                Err(e) => {
                    eprintln!(
                        "    {} Failed to archive {} ({})",
                        "Error".red().bold(),
                        assessment.path.display(),
                        e
                    );
                }
            }
        }
    } else if safe_count > 0 {
        println!(
            "{}",
            "Dry run. Use --execute to archive safe repos.".yellow()
        );
    }

    Ok(())
}

fn create_archive(source: &Path, dest: &Path) -> Result<()> {
    let file = fs::File::create(dest).context("Failed to create archive file")?;
    let encoder = GzEncoder::new(file, Compression::default());
    let mut archive = tar::Builder::new(encoder);
    let dir_name = source
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "repo".to_string());
    archive
        .append_dir_all(&dir_name, source)
        .context("Failed to add directory to archive")?;
    archive.finish().context("Failed to finalise archive")?;
    Ok(())
}

pub fn restore(name: Option<String>) -> Result<()> {
    let archive_dir = reap_archives_dir();
    if !archive_dir.exists() {
        println!("{}", "No archives directory found.".yellow());
        return Ok(());
    }

    if let Some(name) = name {
        return restore_archive(&archive_dir, &name);
    }

    let mut entries: Vec<(String, ArchiveManifest)> = Vec::new();

    for entry in fs::read_dir(&archive_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            let content = fs::read_to_string(&path)?;
            if let Ok(manifest) = serde_json::from_str::<ArchiveManifest>(&content) {
                let archive_name = path
                    .file_stem()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                entries.push((archive_name, manifest));
            }
        }
    }

    if entries.is_empty() {
        println!("{}", "No archives found.".yellow());
        return Ok(());
    }

    entries.sort_by(|a, b| a.1.archived_at.cmp(&b.1.archived_at));

    println!("{}", "Archives:".bold());
    for (name, manifest) in &entries {
        println!(
            "  {} (from {}, archived {}, {})",
            name.cyan(),
            manifest.original_path,
            manifest.archived_at.dimmed(),
            format_size(manifest.size_bytes),
        );
    }
    println!();
    println!("Use {} to restore an archive.", "reap restore <name>".bold());

    Ok(())
}

fn restore_archive(archive_dir: &Path, name: &str) -> Result<()> {
    let archive_path = if name.ends_with(".tar.gz") {
        archive_dir.join(name)
    } else {
        archive_dir.join(format!("{name}.tar.gz"))
    };

    let manifest_path = if name.ends_with(".tar.gz") {
        archive_dir.join(format!("{name}.json"))
    } else {
        archive_dir.join(format!("{name}.tar.gz.json"))
    };

    if !archive_path.exists() {
        anyhow::bail!("Archive not found: {}", archive_path.display());
    }

    let original_path = if manifest_path.exists() {
        let content = fs::read_to_string(&manifest_path)?;
        let manifest: ArchiveManifest = serde_json::from_str(&content)?;
        PathBuf::from(&manifest.original_path)
    } else {
        anyhow::bail!(
            "Manifest not found for archive: {}",
            archive_path.display()
        );
    };

    if original_path.exists() {
        anyhow::bail!(
            "Target path already exists: {}. Remove it first.",
            original_path.display()
        );
    }

    let parent = original_path
        .parent()
        .context("Could not determine parent directory")?;
    fs::create_dir_all(parent)?;

    println!(
        "Restoring {} to {} ...",
        archive_path.display(),
        original_path.display()
    );

    let file = fs::File::open(&archive_path)?;
    let decoder = GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(parent)?;

    fs::remove_file(&archive_path)?;
    if manifest_path.exists() {
        fs::remove_file(&manifest_path)?;
    }

    println!("{} Restored to {}", "Done".green(), original_path.display());
    Ok(())
}
