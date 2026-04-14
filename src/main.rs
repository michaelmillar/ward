mod archive;
mod assess;
mod bundle;
mod cache;
mod clean;
mod config;
mod dedupe;
mod git;
mod manifest;
mod restore;
mod scan;
mod sessions;
mod status;
mod sweep;
mod util;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::config::ExcludeOpts;

#[derive(Parser)]
#[command(
    name = "ward",
    version,
    about = "Git workspace lifecycle manager with provable safety"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Overview of workspace disk usage and repo lifecycle")]
    Status {
        path: Option<PathBuf>,
        #[arg(long, help = "Bypass the repo assessment cache")]
        no_cache: bool,
        #[arg(long, help = "Emit JSON")]
        json: bool,
        #[command(flatten)]
        excludes: ExcludeOpts,
    },

    #[command(about = "Classify each repo with a verdict and rationale")]
    Scan {
        path: Option<PathBuf>,
        #[arg(long, help = "Show only prototype repos")]
        prototypes: bool,
        #[arg(
            long,
            help = "Filter by verdict: archive, prototype, keep, local-work, no-remote"
        )]
        verdict: Option<String>,
        #[arg(long, help = "Bypass the repo assessment cache")]
        no_cache: bool,
        #[arg(long, help = "Emit JSON")]
        json: bool,
        #[command(flatten)]
        excludes: ExcludeOpts,
    },

    #[command(about = "Find duplicate clones and propose worktree conversion plan")]
    Dedupe {
        path: Option<PathBuf>,
        #[arg(long, help = "Execute the conversion plan")]
        convert: bool,
        #[command(flatten)]
        excludes: ExcludeOpts,
    },

    #[command(about = "Bundle, verify, and archive stale git repos with safety proofs")]
    Archive {
        path: Option<PathBuf>,
        #[arg(long, help = "Actually archive (default is dry run)")]
        execute: bool,
        #[arg(long, help = "Also include prototype repos")]
        prototypes: bool,
        #[arg(long, help = "Also include repos without a remote")]
        include_no_remote: bool,
        #[arg(long, help = "Bypass the repo assessment cache")]
        no_cache: bool,
        #[arg(long, help = "Emit JSON of eligible repos")]
        json: bool,
        #[command(flatten)]
        excludes: ExcludeOpts,
    },

    #[command(about = "Manage ward configuration")]
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    #[command(about = "Clear the assessment cache")]
    CacheClear,

    #[command(about = "List or restore archived repos with integrity verification")]
    Restore {
        name: Option<String>,
        #[arg(long, help = "Verify archive integrity without restoring")]
        verify: bool,
    },

    #[command(about = "Find and remove regenerable build artefacts")]
    Clean {
        path: Option<PathBuf>,
        #[arg(long, help = "Actually delete artefacts (default is dry run)")]
        execute: bool,
        #[arg(
            long,
            help = "Only artefacts in projects not modified in N days (e.g. 30d, 4w)"
        )]
        older_than: Option<String>,
        #[command(flatten)]
        excludes: ExcludeOpts,
    },

    #[command(about = "Bulk reclaim: clean + archive in one pass")]
    Sweep {
        path: Option<PathBuf>,
        #[arg(long, help = "Actually execute (default is dry run)")]
        execute: bool,
        #[arg(long, help = "Include prototype repos in archive step")]
        prototypes: bool,
        #[arg(long, help = "Only artefacts in projects not modified in N days")]
        older_than: Option<String>,
        #[command(flatten)]
        excludes: ExcludeOpts,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    #[command(about = "Write a default config file to ~/.ward/config.toml")]
    Init,
    #[command(about = "Print the effective configuration")]
    Show,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Status {
            path,
            no_cache,
            json,
            excludes,
        } => status::run(path, no_cache, json, excludes),
        Commands::Scan {
            path,
            prototypes,
            verdict,
            no_cache,
            json,
            excludes,
        } => scan::run(path, prototypes, verdict, no_cache, json, excludes),
        Commands::Dedupe {
            path,
            convert,
            excludes,
        } => dedupe::run(path, convert, excludes),
        Commands::Archive {
            path,
            execute,
            prototypes,
            include_no_remote,
            no_cache,
            json,
            excludes,
        } => archive::run(
            path,
            execute,
            prototypes,
            include_no_remote,
            no_cache,
            json,
            excludes,
        ),
        Commands::Restore { name, verify } => restore::run(name, verify),
        Commands::Clean {
            path,
            execute,
            older_than,
            excludes,
        } => clean::run(path, execute, older_than, excludes),
        Commands::Sweep {
            path,
            execute,
            prototypes,
            older_than,
            excludes,
        } => sweep::run(path, execute, prototypes, older_than, excludes),
        Commands::Config { action } => match action {
            ConfigAction::Init => config::init_command(),
            ConfigAction::Show => config::show_command(),
        },
        Commands::CacheClear => {
            cache::Cache::clear()?;
            println!("Cache cleared.");
            Ok(())
        }
    }
}
