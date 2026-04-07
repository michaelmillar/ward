use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::util::ward_home;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Config {
    #[serde(default)]
    pub workspace: Workspace,
    #[serde(default)]
    pub thresholds: Thresholds,
    #[serde(default)]
    pub artefact_rules: Vec<CustomArtefactRule>,
    #[serde(default)]
    pub exclude: Exclude,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Workspace {
    pub root: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Thresholds {
    pub archive_stale_days: i64,
    pub prototype_max_commits: u64,
    pub prototype_max_authors: u64,
    pub prototype_max_lifetime_days: i64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            archive_stale_days: 90,
            prototype_max_commits: 10,
            prototype_max_authors: 1,
            prototype_max_lifetime_days: 30,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CustomArtefactRule {
    pub name: String,
    pub ecosystem: String,
    #[serde(default)]
    pub requires_sibling: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Exclude {
    #[serde(default)]
    pub paths: Vec<String>,
}


impl Config {
    pub fn path() -> PathBuf {
        ward_home().join("config.toml")
    }

    pub fn load() -> Self {
        let path = Self::path();
        if !path.exists() {
            return Self::default();
        }
        match fs::read_to_string(&path) {
            Ok(text) => match toml::from_str::<Config>(&text) {
                Ok(cfg) => cfg,
                Err(e) => {
                    eprintln!(
                        "warning: could not parse {} ({}), using defaults",
                        path.display(),
                        e
                    );
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self).context("Failed to serialise config")?;
        fs::write(&path, text)
            .with_context(|| format!("Failed to write config to {}", path.display()))?;
        Ok(())
    }

    pub fn workspace_root(&self) -> Option<PathBuf> {
        self.workspace
            .root
            .as_ref()
            .map(|s| expand_tilde(s))
    }

    pub fn is_excluded(&self, path: &Path) -> bool {
        self.exclude
            .paths
            .iter()
            .map(|s| expand_tilde(s))
            .any(|p| path.starts_with(&p))
    }
}

fn expand_tilde(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/") {
        crate::util::dirs_home().join(rest)
    } else if s == "~" {
        crate::util::dirs_home()
    } else {
        PathBuf::from(s)
    }
}

pub fn init_command() -> Result<()> {
    let path = Config::path();
    if path.exists() {
        println!("Config already exists at {}", path.display());
        return Ok(());
    }
    let default = Config::default();
    default.save()?;
    println!("Wrote default config to {}", path.display());
    Ok(())
}

pub fn show_command() -> Result<()> {
    let cfg = Config::load();
    let text = toml::to_string_pretty(&cfg)?;
    println!("{text}");
    println!("# loaded from {}", Config::path().display());
    Ok(())
}
