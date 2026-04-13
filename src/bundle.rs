use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::Command;

pub fn create(repo: &Path, dest: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["bundle", "create"])
        .arg(dest)
        .arg("--all")
        .current_dir(repo)
        .output()
        .context("Failed to run git bundle create")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).to_string();
        bail!("git bundle create failed: {err}");
    }
    if !dest.exists() {
        bail!("Bundle was not created at {}", dest.display());
    }
    Ok(())
}

pub fn verify(bundle: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["bundle", "verify"])
        .arg(bundle)
        .output()
        .context("Failed to run git bundle verify")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).to_string();
        bail!("git bundle verify failed: {err}");
    }
    Ok(())
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file =
        fs::File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn restore_clone(bundle: &Path, target: &Path) -> Result<()> {
    let parent = target
        .parent()
        .context("Could not determine parent directory for restore target")?;
    fs::create_dir_all(parent)?;
    let output = Command::new("git")
        .arg("clone")
        .arg(bundle)
        .arg(target)
        .output()
        .context("Failed to run git clone from bundle")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).to_string();
        bail!("git clone from bundle failed: {err}");
    }
    Ok(())
}

pub fn fetch_custom_refs(bundle: &Path, repo: &Path, refspec: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["fetch"])
        .arg(bundle)
        .arg(format!("{refspec}:{refspec}"))
        .current_dir(repo)
        .output()
        .context("Failed to fetch custom refs from bundle")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).to_string();
        bail!("fetch custom refs failed: {err}");
    }
    Ok(())
}

pub fn verify_by_clone(bundle: &Path) -> Result<VerifiedRefs> {
    let tmp = tempfile::tempdir().context("Failed to create temp dir for verify")?;
    let target = tmp.path().join("verify");
    let output = Command::new("git")
        .arg("clone")
        .arg("--mirror")
        .arg(bundle)
        .arg(&target)
        .output()
        .context("Failed to run git clone for verification")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).to_string();
        bail!("verification clone failed: {err}");
    }

    let refs_output = Command::new("git")
        .args(["for-each-ref", "--format=%(refname) %(objectname)"])
        .current_dir(&target)
        .output()
        .context("Failed to list refs in verification clone")?;
    let head_output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&target)
        .output()
        .ok();

    let refs_text = String::from_utf8_lossy(&refs_output.stdout).to_string();
    let mut refs = Vec::new();
    for line in refs_text.lines() {
        let mut parts = line.splitn(2, ' ');
        let name = parts.next().unwrap_or("").to_string();
        let sha = parts.next().unwrap_or("").to_string();
        if !name.is_empty() && !sha.is_empty() {
            refs.push((name, sha));
        }
    }

    let head = head_output
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());

    Ok(VerifiedRefs { refs, head })
}

pub struct VerifiedRefs {
    pub refs: Vec<(String, String)>,
    pub head: Option<String>,
}
