use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

pub const MANIFEST_VERSION: u32 = 3;

#[derive(Serialize, Deserialize, Clone)]
pub struct Manifest {
    pub manifest_version: u32,
    pub format: ArchiveFormat,
    pub original_path: String,
    pub archived_at: String,
    pub archived_by: String,
    pub size_bytes: u64,
    pub bundle_sha256: Option<String>,
    pub extras_sha256: Option<String>,
    pub head_sha: Option<String>,
    pub refs: Vec<RefEntry>,
    pub remotes: Vec<RemoteEntry>,
    pub first_commit: Option<String>,
    pub last_commit: Option<String>,
    pub commit_count: u64,
    pub verified_at: Option<String>,
    pub verifier_version: Option<String>,
    pub has_extras: bool,
    pub stash_count: u64,
    pub stash_shas: Vec<String>,
    pub tag_count: u64,
    pub has_hooks: bool,
    pub has_config: bool,
    #[serde(default)]
    pub has_config_worktree: bool,
    pub submodule_count: u64,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ArchiveFormat {
    Bundle,
    Tarball,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RefEntry {
    pub name: String,
    pub sha: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RemoteEntry {
    pub name: String,
    pub url: String,
}

pub fn write(manifest: &Manifest, path: &Path) -> Result<()> {
    let json = serde_json::to_string_pretty(manifest).context("Failed to serialise manifest")?;
    fs::write(path, json)
        .with_context(|| format!("Failed to write manifest to {}", path.display()))?;
    Ok(())
}

pub fn read(path: &Path) -> Result<Manifest> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read manifest {}", path.display()))?;
    let manifest: Manifest = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse manifest {}", path.display()))?;
    Ok(manifest)
}
