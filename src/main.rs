use crate::commands::{handle_command, Commands};
use anyhow::Context;
use clap::{CommandFactory, Parser};
use std::sync::OnceLock;
use std::{collections::BTreeMap, fs, path::PathBuf};

pub use crate::projects::*;

mod commands;
mod docker;
mod lock;
mod progress;
mod projects;
mod spinner;

pub static PROJECTS: OnceLock<BTreeMap<String, Project>> = OnceLock::new();

fn clap_parse_selector(s: &str) -> Result<TargetSelector, String> {
    parse_selector(
        s,
        PROJECTS
            .get()
            .expect("PROJECTS not initialized"),
    )
    .map_err(|e| e.to_string())
}

fn clap_parse_service_selector(s: &str) -> Result<ServiceSelector, String> {
    parse_service_selector(
        s,
        PROJECTS
            .get()
            .expect("PROJECTS not initialized"),
    )
    .map_err(|e| e.to_string())
}

#[derive(Parser, Debug)]
#[command(
    disable_help_flag = true,
    disable_version_flag = true,
    allow_hyphen_values = true,
    ignore_errors = true
)]
struct CoreCli {
    #[command(flatten)]
    files: FileCli,

    #[clap(trailing_var_arg = true)]
    args: Vec<String>,
}

#[derive(clap::Args, Debug)]
struct FileCli {
    /// Path to the lock file
    #[arg(long, env = "NIRION_LOCK_FILE", hide_env_values = true)]
    lock_file: Option<PathBuf>,

    /// Path to the project file
    #[arg(long, env = "NIRION_PROJECT_FILE", hide_env_values = true)]
    project_file: Option<PathBuf>,

    #[arg(long, conflicts_with = "project_file")]
    nix_eval: bool,

    #[arg(long)]
    nix_eval_target: Option<String>,
}

#[derive(Parser)]
#[command(name = "nirion")]
struct Cli {
    #[command(flatten)]
    files: FileCli,

    #[command(subcommand)]
    command: Commands,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let core_cli = CoreCli::parse();

    let Some(lock_file) = core_cli.files.lock_file else {
        eprintln!("No lock file specified\n");
        Cli::command().print_help()?;
        std::process::exit(0)
    };

    let locked_images: BTreeMap<String, String> = if lock_file.exists() {
        let lock_file_data = fs::read_to_string(&lock_file)
            .with_context(|| anyhow::anyhow!("Failed to read lock file"))?;
        serde_json::from_str(&lock_file_data)
            .with_context(|| anyhow::anyhow!("Failed to parse lock file"))?
    } else {
        BTreeMap::new()
    };

    let Some(project_file) = core_cli.files.project_file else {
        eprintln!("No project file specified\n");
        Cli::command().print_help()?;
        std::process::exit(0)
    };

    let project_data = fs::read_to_string(&project_file)
        .with_context(|| anyhow::anyhow!("Failed to read projects file"))?;
    let projects: BTreeMap<String, Project> =
        serde_json::from_str(&project_data).with_context(|| {
            anyhow::anyhow!("Failed to parse projects file")
        })?;

    PROJECTS
        .set(projects)
        .map_err(|_| anyhow::anyhow!("PROJECTS already initialized"))?;

    let mut args = core_cli.args;
    args.insert(0, Cli::command().get_name().to_string());

    let cli = Cli::parse_from(args);

    let projects = PROJECTS
        .get()
        .ok_or_else(|| anyhow::anyhow!("PROJECTS not initialized"))?;

    handle_command(&cli.command, &projects, &locked_images, &lock_file).await?;

    Ok(())
}
