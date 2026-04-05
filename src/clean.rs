use anyhow::Result;
use chrono::{NaiveDate, Utc};
use colored::Colorize;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::scan::{dir_size, format_size};

struct ArtifactRule {
    name: &'static str,
    requires_sibling: Option<&'static [&'static str]>,
}

const ARTIFACT_RULES: &[ArtifactRule] = &[
    ArtifactRule {
        name: "target",
        requires_sibling: None,
    },
    ArtifactRule {
        name: "node_modules",
        requires_sibling: None,
    },
    ArtifactRule {
        name: ".next",
        requires_sibling: None,
    },
    ArtifactRule {
        name: "dist",
        requires_sibling: Some(&["package.json", "webpack.config.js", "vite.config.ts", "vite.config.js"]),
    },
    ArtifactRule {
        name: "__pycache__",
        requires_sibling: None,
    },
    ArtifactRule {
        name: ".gradle",
        requires_sibling: None,
    },
    ArtifactRule {
        name: "build",
        requires_sibling: Some(&["build.gradle", "build.gradle.kts", "CMakeLists.txt"]),
    },
];

struct Artifact {
    path: PathBuf,
    size: u64,
    last_modified: Option<NaiveDate>,
}

fn has_sibling(dir: &Path, siblings: &[&str]) -> bool {
    let Some(parent) = dir.parent() else {
        return false;
    };
    siblings.iter().any(|s| parent.join(s).exists())
}

fn project_last_modified(dir: &Path) -> Option<NaiveDate> {
    let parent = dir.parent()?;
    let git_dir = parent.join(".git");
    if git_dir.exists() {
        return crate::scan::last_commit_date(parent);
    }
    let meta = fs::metadata(parent).ok()?;
    let modified = meta.modified().ok()?;
    let dt: chrono::DateTime<Utc> = modified.into();
    Some(dt.date_naive())
}

fn find_artifacts(root: &Path) -> Vec<Artifact> {
    let mut artifacts = Vec::new();
    let skip_dirs: Vec<&str> = ARTIFACT_RULES.iter().map(|r| r.name).collect();

    let walker = WalkDir::new(root).into_iter();
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

        let name = entry.file_name().to_string_lossy();

        if let Some(parent) = entry.path().parent() {
            let parent_name = parent.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if skip_dirs.contains(&parent_name.as_str()) {
                continue;
            }
        }

        for rule in ARTIFACT_RULES {
            if name != rule.name {
                continue;
            }
            if let Some(siblings) = rule.requires_sibling {
                if !has_sibling(entry.path(), siblings) {
                    continue;
                }
            }
            let size = dir_size(entry.path());
            let last_modified = project_last_modified(entry.path());
            artifacts.push(Artifact {
                path: entry.path().to_path_buf(),
                size,
                last_modified,
            });
        }
    }

    artifacts.sort_by(|a, b| b.size.cmp(&a.size));
    artifacts
}

fn parse_duration(s: &str) -> Result<i64> {
    let s = s.trim();
    if let Some(days) = s.strip_suffix('d') {
        Ok(days.parse::<i64>()?)
    } else if let Some(weeks) = s.strip_suffix('w') {
        Ok(weeks.parse::<i64>()? * 7)
    } else {
        anyhow::bail!("Invalid duration format '{}'. Use e.g. 30d or 4w", s)
    }
}

pub fn run(path: Option<PathBuf>, execute: bool, older_than: Option<String>) -> Result<()> {
    let root = path.unwrap_or_else(crate::scan::default_projects_path);
    if !root.exists() {
        anyhow::bail!("Path does not exist: {}", root.display());
    }

    let cutoff = if let Some(ref duration) = older_than {
        let days = parse_duration(duration)?;
        let date = Utc::now().date_naive() - chrono::Duration::days(days);
        Some(date)
    } else {
        None
    };

    println!(
        "{}",
        format!("Scanning {} for build artefacts...", root.display()).dimmed()
    );

    let mut artifacts = find_artifacts(&root);

    if let Some(cutoff_date) = cutoff {
        artifacts.retain(|a| {
            a.last_modified
                .map(|d| d < cutoff_date)
                .unwrap_or(true)
        });
    }

    if artifacts.is_empty() {
        println!("{}", "No reclaimable artefacts found.".green());
        return Ok(());
    }

    let total: u64 = artifacts.iter().map(|a| a.size).sum();

    for artifact in &artifacts {
        let size_str = format_size(artifact.size);
        let date_str = artifact
            .last_modified
            .map(|d| d.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let colour = if artifact.size > 1024 * 1024 * 1024 {
            size_str.red()
        } else if artifact.size > 100 * 1024 * 1024 {
            size_str.yellow()
        } else {
            size_str.green()
        };

        println!(
            "  {} {} (last modified: {})",
            colour,
            artifact.path.display(),
            date_str.dimmed()
        );
    }

    println!();
    println!(
        "Total reclaimable: {} across {} artefact(s)",
        format_size(total).bold(),
        artifacts.len()
    );

    if execute {
        println!();
        println!("{}", "Removing artefacts...".red().bold());
        let mut freed = 0u64;
        for artifact in &artifacts {
            match fs::remove_dir_all(&artifact.path) {
                Ok(()) => {
                    freed += artifact.size;
                    println!("  {} {}", "Removed".red(), artifact.path.display());
                }
                Err(e) => {
                    eprintln!(
                        "  {} {} ({})",
                        "Failed".red().bold(),
                        artifact.path.display(),
                        e
                    );
                }
            }
        }
        println!();
        println!("{} freed", format_size(freed).green().bold());
    } else {
        println!();
        println!(
            "{}",
            "Dry run. Use --execute to actually remove artefacts.".yellow()
        );
    }

    Ok(())
}
