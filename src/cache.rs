use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::assess::Assessment;
use crate::util::ward_home;

#[derive(Serialize, Deserialize, Default)]
pub struct Cache {
    entries: HashMap<String, Entry>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Entry {
    pub git_mtime_secs: u64,
    pub assessment: Assessment,
}

impl Cache {
    pub fn path() -> PathBuf {
        ward_home().join("cache.json")
    }

    pub fn load() -> Self {
        let path = Self::path();
        if !path.exists() {
            return Self::default();
        }
        match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string(self)?;
        fs::write(&path, text)?;
        Ok(())
    }

    pub fn lookup(&self, repo: &Path) -> Option<&Assessment> {
        let mtime = git_mtime_secs(repo)?;
        let key = repo.display().to_string();
        let entry = self.entries.get(&key)?;
        if entry.git_mtime_secs == mtime {
            Some(&entry.assessment)
        } else {
            None
        }
    }

    pub fn store(&mut self, repo: &Path, assessment: Assessment) {
        let Some(mtime) = git_mtime_secs(repo) else {
            return;
        };
        let key = repo.display().to_string();
        self.entries.insert(
            key,
            Entry {
                git_mtime_secs: mtime,
                assessment,
            },
        );
    }

    pub fn clear() -> Result<()> {
        let path = Self::path();
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

pub fn git_mtime_secs(repo: &Path) -> Option<u64> {
    let meta = fs::metadata(repo.join(".git")).ok()?;
    let mtime = meta.modified().ok()?;
    let secs = mtime.duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(secs)
}

