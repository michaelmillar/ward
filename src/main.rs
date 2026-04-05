mod archive;
mod clean;
mod dupes;
mod scan;
mod status;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "reap", about = "Disk space reclaimer for developers")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Find and remove regenerable build artefacts")]
    Clean {
        path: Option<PathBuf>,
        #[arg(long, help = "Actually delete artefacts (default is dry run)")]
        execute: bool,
        #[arg(long, help = "Only show artefacts in projects not modified in N days (e.g. 30d, 4w)")]
        older_than: Option<String>,
    },
    #[command(about = "Find duplicate git repos with the same remote")]
    Dupes {
        path: Option<PathBuf>,
    },
    #[command(about = "Assess and archive stale git repos")]
    Archive {
        path: Option<PathBuf>,
        #[arg(long, help = "Actually archive safe repos (default is dry run)")]
        execute: bool,
    },
    #[command(about = "Restore a previously archived repo")]
    Restore {
        name: Option<String>,
    },
    #[command(about = "Overview of disk usage across projects")]
    Status {
        path: Option<PathBuf>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Clean {
            path,
            execute,
            older_than,
        } => clean::run(path, execute, older_than),
        Commands::Dupes { path } => dupes::run(path),
        Commands::Archive { path, execute } => archive::run(path, execute),
        Commands::Restore { name } => archive::restore(name),
        Commands::Status { path } => status::run(path),
    }
}
