use anyhow::{Result, bail};
use colored::Colorize;
use std::path::PathBuf;

use crate::util::{default_projects_path, format_size};
use crate::{archive, clean};

pub fn run(
    path: Option<PathBuf>,
    execute: bool,
    include_prototypes: bool,
    older_than: Option<String>,
) -> Result<()> {
    let root = path.clone().unwrap_or_else(default_projects_path);
    if !root.exists() {
        bail!("Path does not exist: {}", root.display());
    }

    println!("{}", "ward sweep".bold().underline());
    println!("{}", format!("  root {}", root.display()).dimmed());
    println!(
        "{}",
        format!("  mode {}", if execute { "execute" } else { "dry run" }).dimmed()
    );
    println!();

    let reclaimable = clean::total_reclaimable(&root);
    println!(
        "{} {} of build artefacts reclaimable",
        "step 1".bold(),
        format_size(reclaimable).green().bold()
    );
    println!();

    println!("{}", "step 1   clean".bold());
    clean::run(Some(root.clone()), execute, older_than.clone())?;
    println!();

    println!("{}", "step 2   archive".bold());
    archive::run(path, execute, include_prototypes, false, false, false)?;

    Ok(())
}
