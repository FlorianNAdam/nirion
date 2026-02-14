use crate::commands::{handle_command, Commands};
use anyhow::Context;
use clap::{CommandFactory, Parser};
use clap_complete::{ArgValueCompleter, CompletionCandidate};
use crossterm::style::Stylize;
use nirion_lib::lock::LockedImages;
use nirion_lib::projects::{
    parse_selector, parse_service_selector, Project, Projects, ServiceSelector,
    TargetSelector,
};
use std::sync::OnceLock;
use std::{fs, path::PathBuf};
use tokio::process::Command;

mod commands;
mod docker;
mod lock;
mod monitor;
mod progress;

pub static PROJECTS: OnceLock<Projects> = OnceLock::new();

pub trait ClapSelector {
    fn clap_parse(s: &str) -> Result<Self, String>
    where
        Self: Sized;

    fn clap_completer() -> ArgValueCompleter;
}

impl ClapSelector for TargetSelector {
    fn clap_parse(s: &str) -> Result<Self, String> {
        parse_selector(
            s,
            PROJECTS
                .get()
                .expect("PROJECTS not initialized"),
        )
        .map_err(|e| e.to_string())
    }

    fn clap_completer() -> ArgValueCompleter {
        ArgValueCompleter::new(target_selector_completer)
    }
}

pub fn target_selector_completer(
    current: &std::ffi::OsStr,
) -> Vec<CompletionCandidate> {
    let core_cli = CoreCli::parse();

    let projects = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current()
            .block_on(core_cli.files.get_projects())
    })
    .unwrap_or_default();

    let mut completions = vec![];

    let Some(current) = current.to_str() else {
        return completions;
    };

    if "*".starts_with(current) {
        completions.push(CompletionCandidate::new("*"));
    }

    for (project_name, project) in projects.iter() {
        if let Some(pos) = current.find('.') {
            let (proj_prefix, svc_prefix) = current.split_at(pos);
            let svc_prefix = &svc_prefix[1..];

            if project_name.starts_with(proj_prefix) {
                for service_name in project.services.keys() {
                    if service_name.starts_with(svc_prefix) {
                        completions.push(CompletionCandidate::new(format!(
                            "{}.{}",
                            proj_prefix, service_name
                        )));
                    }
                }
            }
        } else {
            if project_name.starts_with(current) {
                completions.push(CompletionCandidate::new(project_name));
            }
        }
    }

    completions
}

impl ClapSelector for ServiceSelector {
    fn clap_parse(s: &str) -> Result<Self, String> {
        parse_service_selector(
            s,
            PROJECTS
                .get()
                .expect("PROJECTS not initialized"),
        )
        .map_err(|e| e.to_string())
    }

    fn clap_completer() -> ArgValueCompleter {
        ArgValueCompleter::new(service_selector_completer)
    }
}

pub fn service_selector_completer(
    current: &std::ffi::OsStr,
) -> Vec<CompletionCandidate> {
    let core_cli = CoreCli::parse();

    let projects = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current()
            .block_on(core_cli.files.get_projects())
    })
    .unwrap_or_default();

    let mut completions = vec![];

    let Some(current) = current.to_str() else {
        return completions;
    };

    let (proj_prefix, svc_prefix) = if let Some(pos) = current.find('.') {
        let (p, s) = current.split_at(pos);
        (p, &s[1..])
    } else {
        (current, "")
    };

    for (project_name, project) in projects.iter() {
        if project_name.starts_with(proj_prefix) {
            for service_name in project.services.keys() {
                if service_name.starts_with(svc_prefix) {
                    completions.push(CompletionCandidate::new(format!(
                        "{}.{}",
                        project_name, service_name
                    )));
                }
            }
        }
    }

    completions
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

    /// Evaluate a nix target to build the project file
    #[arg(long, conflicts_with = "project_file")]
    nix_eval: bool,

    /// A nix target to evaluate
    #[arg(
        long,
        env = "NIX_TARGET",
        hide_env_values = true,
        conflicts_with = "raw_nix_target"
    )]
    nix_target: Option<String>,

    /// A raw nix target to evaluate
    #[arg(
        long,
        env = "RAW_NIX_TARGET",
        hide_env_values = true,
        conflicts_with = "nix_target"
    )]
    raw_nix_target: Option<String>,
}

impl FileCli {
    async fn get_lock_file(&self) -> anyhow::Result<PathBuf> {
        if let Some(file) = &self.lock_file {
            Ok(file.clone())
        } else {
            anyhow::bail!(
                "{}\n\n{}",
                "No lock file specified".red(),
                Cli::command().render_help().ansi()
            )
        }
    }

    async fn get_locked_images(&self) -> anyhow::Result<LockedImages> {
        let lock_file = self.get_lock_file().await?;

        let locked_images: LockedImages = if lock_file.exists() {
            let lock_file_data = fs::read_to_string(&lock_file)
                .with_context(|| anyhow::anyhow!("Failed to read lock file"))?;
            serde_json::from_str(&lock_file_data)
                .with_context(|| anyhow::anyhow!("Failed to parse lock file"))?
        } else {
            LockedImages::default()
        };

        Ok(locked_images)
    }

    async fn get_project_file(&self) -> anyhow::Result<PathBuf> {
        if self.nix_eval {
            let nix_eval_target = self
                .nix_target
                .as_ref()
                .map(&String::as_str)
                .map(get_nix_target)
                .or_else(|| {
                    self.raw_nix_target
                        .as_ref()
                        .map(|t| t.to_string())
                })
                .ok_or_else(|| anyhow::anyhow!("No nix target specified"))?;
            build_nix_project_file(&nix_eval_target).await
        } else if let Some(project_file) = &self.project_file {
            Ok(project_file.clone())
        } else {
            anyhow::bail!(
                "{}\n\n{}",
                "No project file specified".red(),
                Cli::command().render_help().ansi()
            )
        }
    }

    async fn get_projects(&self) -> anyhow::Result<Projects> {
        let project_file = self.get_project_file().await?;

        let project_data = fs::read_to_string(&project_file)
            .with_context(|| anyhow::anyhow!("Failed to read projects file"))?;
        let projects: Projects = serde_json::from_str(&project_data)
            .with_context(|| {
                anyhow::anyhow!("Failed to parse projects file")
            })?;

        Ok(projects)
    }
}

#[derive(Parser)]
#[command(name = "nirion")]
struct Cli {
    #[command(flatten)]
    files: FileCli,

    #[command(subcommand)]
    command: Commands,
}

pub fn get_nix_target(target: &str) -> String {
    format!(
        "{}.{}",
        target,
        ["config", "virtualisation", "nirion", "out", "projectsFile"].join(".")
    )
}

pub async fn build_nix_project_file(
    nix_eval_target: &str,
) -> anyhow::Result<PathBuf> {
    let output = Command::new("nix")
        .args(["build", &nix_eval_target, "--no-link", "--print-out-paths"])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("{}", stderr);
        anyhow::bail!("nix build failed with status {}", output.status);
    }

    let raw_path = str::from_utf8(&output.stdout)?
        .trim()
        .to_string();

    let path = PathBuf::from(raw_path);

    Ok(path)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    clap_complete::CompleteEnv::with_factory(|| Cli::command()).complete();

    let core_cli = CoreCli::parse();

    let lock_file = core_cli.files.get_lock_file().await?;
    let locked_images = core_cli
        .files
        .get_locked_images()
        .await?;

    let projects = core_cli.files.get_projects().await?;

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
