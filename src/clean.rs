use anyhow::{Result, bail};
use chrono::{NaiveDate, Utc};
use colored::Colorize;
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::config::Config;
use crate::git;
use crate::util::{default_projects_path, dir_size, format_size};

struct ArtifactRule {
    name: String,
    ecosystem: String,
    requires_sibling: Vec<String>,
}

const BUILTIN_RULES: &[BuiltinRule] = &[
    BuiltinRule { name: "target", ecosystem: "rust", requires_sibling: Some(&["Cargo.toml"]) },
    BuiltinRule { name: "node_modules", ecosystem: "node", requires_sibling: None },
    BuiltinRule { name: ".next", ecosystem: "nextjs", requires_sibling: None },
    BuiltinRule { name: ".turbo", ecosystem: "turbo", requires_sibling: None },
    BuiltinRule { name: ".vite", ecosystem: "vite", requires_sibling: None },
    BuiltinRule { name: ".parcel-cache", ecosystem: "parcel", requires_sibling: None },
    BuiltinRule { name: ".swc", ecosystem: "swc", requires_sibling: None },
    BuiltinRule { name: ".pnpm-store", ecosystem: "pnpm", requires_sibling: None },
    BuiltinRule {
        name: "dist",
        ecosystem: "node",
        requires_sibling: Some(&[
            "package.json",
            "webpack.config.js",
            "vite.config.ts",
            "vite.config.js",
            "rollup.config.js",
        ]),
    },
    BuiltinRule {
        name: "build",
        ecosystem: "gradle/cmake",
        requires_sibling: Some(&["build.gradle", "build.gradle.kts", "CMakeLists.txt"]),
    },
    BuiltinRule { name: ".gradle", ecosystem: "gradle", requires_sibling: None },
    BuiltinRule { name: "__pycache__", ecosystem: "python", requires_sibling: None },
    BuiltinRule { name: ".venv", ecosystem: "python", requires_sibling: None },
    BuiltinRule { name: ".export-venv", ecosystem: "python", requires_sibling: None },
    BuiltinRule {
        name: "venv",
        ecosystem: "python",
        requires_sibling: Some(&["pyproject.toml", "requirements.txt", "setup.py"]),
    },
    BuiltinRule { name: ".ruff_cache", ecosystem: "python", requires_sibling: None },
    BuiltinRule { name: ".mypy_cache", ecosystem: "python", requires_sibling: None },
    BuiltinRule { name: ".pytest_cache", ecosystem: "python", requires_sibling: None },
    BuiltinRule { name: ".tox", ecosystem: "python", requires_sibling: None },
    BuiltinRule { name: ".nox", ecosystem: "python", requires_sibling: None },
    BuiltinRule { name: "out", ecosystem: "go", requires_sibling: Some(&["go.mod"]) },
    BuiltinRule { name: "zig-cache", ecosystem: "zig", requires_sibling: None },
    BuiltinRule { name: "zig-out", ecosystem: "zig", requires_sibling: None },
    BuiltinRule { name: ".dart_tool", ecosystem: "dart", requires_sibling: None },
    BuiltinRule { name: "DerivedData", ecosystem: "xcode", requires_sibling: None },
];

struct BuiltinRule {
    name: &'static str,
    ecosystem: &'static str,
    requires_sibling: Option<&'static [&'static str]>,
}

fn all_rules(cfg: &Config) -> Vec<ArtifactRule> {
    let mut rules: Vec<ArtifactRule> = BUILTIN_RULES
        .iter()
        .map(|r| ArtifactRule {
            name: r.name.to_string(),
            ecosystem: r.ecosystem.to_string(),
            requires_sibling: r
                .requires_sibling
                .map(|s| s.iter().map(|x| x.to_string()).collect())
                .unwrap_or_default(),
        })
        .collect();
    for custom in &cfg.artefact_rules {
        rules.push(ArtifactRule {
            name: custom.name.clone(),
            ecosystem: custom.ecosystem.clone(),
            requires_sibling: custom.requires_sibling.clone(),
        });
    }
    rules
}

struct Artifact {
    path: PathBuf,
    size: u64,
    ecosystem: String,
    last_modified: Option<NaiveDate>,
}

fn has_sibling(dir: &Path, siblings: &[String]) -> bool {
    let Some(parent) = dir.parent() else {
        return false;
    };
    siblings.iter().any(|s| parent.join(s).exists())
}

fn project_last_modified(dir: &Path) -> Option<NaiveDate> {
    let parent = dir.parent()?;
    let git_dir = parent.join(".git");
    if git_dir.exists() {
        return git::last_commit_date(parent);
    }
    let meta = fs::metadata(parent).ok()?;
    let modified = meta.modified().ok()?;
    let dt: chrono::DateTime<Utc> = modified.into();
    Some(dt.date_naive())
}

fn find_artifacts(root: &Path, rules: &[ArtifactRule]) -> Vec<Artifact> {
    let skip_dirs: std::collections::HashSet<&str> =
        rules.iter().map(|r| r.name.as_str()).collect();

    let mut candidates: Vec<(PathBuf, String)> = Vec::new();
    let mut it = WalkDir::new(root).into_iter();
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

        if name.starts_with('.') && entry.depth() > 0 && !skip_dirs.contains(name.as_str()) {
            it.skip_current_dir();
            continue;
        }

        let mut matched = false;
        for rule in rules {
            if name != rule.name {
                continue;
            }
            if !rule.requires_sibling.is_empty()
                && !has_sibling(entry.path(), &rule.requires_sibling)
            {
                continue;
            }
            candidates.push((entry.path().to_path_buf(), rule.ecosystem.clone()));
            matched = true;
            break;
        }
        if matched {
            it.skip_current_dir();
        }
    }

    let mut artifacts: Vec<Artifact> = candidates
        .par_iter()
        .map(|(p, eco)| Artifact {
            path: p.clone(),
            size: dir_size(p),
            ecosystem: eco.clone(),
            last_modified: project_last_modified(p),
        })
        .collect();

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
        bail!("Invalid duration format '{}'. Use e.g. 30d or 4w", s)
    }
}

pub fn run(path: Option<PathBuf>, execute: bool, older_than: Option<String>) -> Result<()> {
    let cfg = Config::load();
    let root = path
        .or_else(|| cfg.workspace_root())
        .unwrap_or_else(default_projects_path);
    if !root.exists() {
        bail!("Path does not exist: {}", root.display());
    }

    let cutoff = if let Some(ref duration) = older_than {
        let days = parse_duration(duration)?;
        Some(Utc::now().date_naive() - chrono::Duration::days(days))
    } else {
        None
    };

    println!(
        "{}",
        format!("Scanning {} for build artefacts ...", root.display()).dimmed()
    );

    let rules = all_rules(&cfg);
    let mut artifacts = find_artifacts(&root, &rules);

    if let Some(cutoff_date) = cutoff {
        artifacts.retain(|a| a.last_modified.map(|d| d < cutoff_date).unwrap_or(true));
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

        let coloured = if artifact.size > 1024 * 1024 * 1024 {
            size_str.red()
        } else if artifact.size > 100 * 1024 * 1024 {
            size_str.yellow()
        } else {
            size_str.green()
        };

        println!(
            "  {:>10} [{:<10}] {} (last modified {})",
            coloured,
            artifact.ecosystem.cyan(),
            artifact.path.display(),
            date_str.dimmed()
        );
    }

    println!();
    println!(
        "Total reclaimable {} across {} artefact(s)",
        format_size(total).bold(),
        artifacts.len()
    );

    if execute {
        println!();
        println!("{}", "Removing artefacts ...".red().bold());
        let mut freed = 0u64;
        for artifact in &artifacts {
            match fs::remove_dir_all(&artifact.path) {
                Ok(()) => {
                    freed += artifact.size;
                    println!("  {} {}", "removed".red(), artifact.path.display());
                }
                Err(e) => {
                    eprintln!(
                        "  {} {} ({})",
                        "failed".red().bold(),
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

pub fn total_reclaimable(root: &Path) -> u64 {
    let cfg = Config::load();
    let rules = all_rules(&cfg);
    find_artifacts(root, &rules).iter().map(|a| a.size).sum()
}
