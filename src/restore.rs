use anyhow::{Context, Result, bail};
use colored::Colorize;
use flate2::read::GzDecoder;
use std::fs;
use std::path::{Path, PathBuf};

use crate::bundle;
use crate::manifest::{self, ArchiveFormat, Manifest};
use crate::util::{archives_dir, format_size};

pub fn run(name: Option<String>, verify_only: bool) -> Result<()> {
    let dir = archives_dir();
    if !dir.exists() {
        println!("{}", "No archives directory found.".yellow());
        return Ok(());
    }

    if let Some(n) = name {
        if verify_only {
            return verify_archive(&dir, &n);
        }
        return restore_one(&dir, &n);
    }

    list(&dir)
}

fn list(dir: &Path) -> Result<()> {
    let mut entries: Vec<(String, Manifest)> = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            if let Ok(m) = manifest::read(&path) {
                let stem = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                entries.push((stem, m));
            }
        }
    }

    if entries.is_empty() {
        println!("{}", "No archives found.".yellow());
        return Ok(());
    }

    entries.sort_by(|a, b| a.1.archived_at.cmp(&b.1.archived_at));

    println!("{}", "Archives".bold().underline());
    for (stem, m) in &entries {
        let format_tag = match m.format {
            ArchiveFormat::Bundle => "bundle".green(),
            ArchiveFormat::Tarball => "tar".yellow(),
        };
        let verified = if m.verified_at.is_some() {
            "verified".green()
        } else {
            "unverified".red()
        };
        println!(
            "  {} [{}] [{}]",
            stem.cyan().bold(),
            format_tag,
            verified
        );
        println!(
            "    from {} ({} refs, {} commits)",
            m.original_path.dimmed(),
            m.refs.len(),
            m.commit_count
        );
        println!(
            "    archived {} original {} {}",
            m.archived_at.dimmed(),
            format_size(m.size_bytes),
            m.head_sha
                .as_deref()
                .map(|s| s.chars().take(10).collect::<String>())
                .unwrap_or_default()
                .dimmed()
        );
    }
    println!();
    println!(
        "Use {} to restore, {} to verify integrity.",
        "ward restore <name>".bold(),
        "ward restore <name> --verify".bold()
    );
    Ok(())
}

fn archive_paths(dir: &Path, name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let stem = name
        .strip_suffix(".json")
        .unwrap_or(name)
        .strip_suffix(".bundle")
        .unwrap_or(name)
        .strip_suffix(".tar.gz")
        .unwrap_or(name)
        .to_string();
    let bundle = dir.join(format!("{stem}.bundle"));
    let tar = dir.join(format!("{stem}.tar.gz"));
    let manifest = dir.join(format!("{stem}.json"));
    if bundle.exists() {
        (bundle, dir.join(format!("{stem}.untracked.tar.gz")), manifest)
    } else {
        (tar, dir.join(format!("{stem}.untracked.tar.gz")), manifest)
    }
}

fn verify_archive(dir: &Path, name: &str) -> Result<()> {
    let (primary, untracked, manifest_path) = archive_paths(dir, name);
    if !manifest_path.exists() {
        bail!("Manifest not found: {}", manifest_path.display());
    }
    if !primary.exists() {
        bail!("Archive not found: {}", primary.display());
    }
    let m = manifest::read(&manifest_path)?;

    println!("Verifying {} ...", primary.display());
    let computed = bundle::sha256_file(&primary)?;
    let expected = match m.format {
        ArchiveFormat::Bundle => m.bundle_sha256.as_deref(),
        ArchiveFormat::Tarball => m.bundle_sha256.as_deref(),
    };
    match expected {
        Some(exp) if exp == computed => {
            println!("  {} archive sha256", "ok".green().bold());
        }
        Some(exp) => {
            bail!("sha256 mismatch. expected {exp}, got {computed}");
        }
        None => {
            println!("  {} no expected hash in manifest", "warn".yellow().bold());
        }
    }

    if m.format == ArchiveFormat::Bundle {
        bundle::verify(&primary).context("git bundle verify")?;
        println!("  {} git bundle verify", "ok".green().bold());
    }

    if m.has_untracked {
        if untracked.exists() {
            let u_sha = bundle::sha256_file(&untracked)?;
            if let Some(exp) = &m.untracked_sha256 {
                if exp == &u_sha {
                    println!("  {} untracked tar sha256", "ok".green().bold());
                } else {
                    bail!("untracked tar sha256 mismatch");
                }
            }
        } else {
            println!(
                "  {} untracked tar missing",
                "warn".yellow().bold()
            );
        }
    }

    println!("{}", "Archive is restorable.".green().bold());
    Ok(())
}

fn restore_one(dir: &Path, name: &str) -> Result<()> {
    let (primary, untracked, manifest_path) = archive_paths(dir, name);
    if !manifest_path.exists() {
        bail!("Manifest not found: {}", manifest_path.display());
    }
    if !primary.exists() {
        bail!("Archive not found: {}", primary.display());
    }
    let m = manifest::read(&manifest_path)?;

    let target = PathBuf::from(&m.original_path);
    if target.exists() {
        bail!(
            "Target path already exists: {}. Remove it first.",
            target.display()
        );
    }

    let expected_hash = m.bundle_sha256.as_deref();
    if let Some(exp) = expected_hash {
        let got = bundle::sha256_file(&primary)?;
        if exp != got {
            bail!("archive sha256 mismatch, refusing to restore");
        }
    }

    println!(
        "Restoring {} to {} ...",
        primary.display(),
        target.display()
    );

    match m.format {
        ArchiveFormat::Bundle => {
            bundle::verify(&primary)?;
            bundle::restore_clone(&primary, &target)?;
            configure_remotes(&target, &m)?;
            if m.has_untracked && untracked.exists() {
                extract_untracked(&untracked, &target)?;
            }
        }
        ArchiveFormat::Tarball => {
            let parent = target
                .parent()
                .context("could not determine parent dir")?;
            fs::create_dir_all(parent)?;
            let file = fs::File::open(&primary)?;
            let decoder = GzDecoder::new(file);
            let mut archive = tar::Archive::new(decoder);
            archive.unpack(parent)?;
        }
    }

    println!("{} restored to {}", "ok".green().bold(), target.display());
    Ok(())
}

fn configure_remotes(repo: &Path, m: &Manifest) -> Result<()> {
    let _ = std::process::Command::new("git")
        .args(["remote", "remove", "origin"])
        .current_dir(repo)
        .output();
    for r in &m.remotes {
        let _ = std::process::Command::new("git")
            .args(["remote", "add", &r.name, &r.url])
            .current_dir(repo)
            .output();
    }
    Ok(())
}

fn extract_untracked(tar_path: &Path, target: &Path) -> Result<()> {
    let file = fs::File::open(tar_path)?;
    let decoder = GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(target)?;
    Ok(())
}
