use crate::commands::{Commands, NirionContext, handle_command};
use clap::{CommandFactory, Parser};
use clap_complete::{ArgValueCompleter, CompletionCandidate};
use crossterm::style::Stylize;
use nirion_lib::config::{
    build_nix_project_file, load_auth_config, load_locked_images,
    load_projects, nix_config_target,
};
use nirion_lib::lock::LockedImages;
use nirion_lib::projects::{
    Project, Projects, ServiceSelector, TargetSelector, parse_selector,
    parse_service_selector,
};
use nirion_oci_lib::client::NirionOciClient;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

mod commands;
mod docker;
mod monitor;
mod progress;
mod status_display;

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
        env = "NIRION_NIX_TARGET",
        hide_env_values = true,
        conflicts_with = "raw_nix_target"
    )]
    nix_target: Option<String>,

    /// A raw nix target to evaluate
    #[arg(
        long,
        env = "NIRION_RAW_NIX_TARGET",
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
        load_locked_images(&lock_file)
    }

    async fn get_project_file(&self) -> anyhow::Result<PathBuf> {
        if self.nix_eval {
            let nix_eval_target = self
                .nix_target
                .as_ref()
                .map(&String::as_str)
                .map(nix_config_target)
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
        load_projects(&project_file)
    }
}

#[derive(Parser)]
#[command(name = "nirion")]
struct Cli {
    #[command(flatten)]
    files: FileCli,

    #[arg(long, env = "NIRION_AUTH_FILE", hide_env_values = true)]
    auth_file: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

impl Cli {
    async fn get_auth(
        &self,
    ) -> anyhow::Result<nirion_oci_lib::client::AuthConfig> {
        load_auth_config(self.auth_file.as_deref())
    }
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
        .set(projects.clone())
        .map_err(|_| anyhow::anyhow!("PROJECTS already initialized"))?;

    let mut args = core_cli.args;
    args.insert(0, Cli::command().get_name().to_string());

    let cli = Cli::parse_from(args);

    let auth = cli.get_auth().await?;
    let oci_client = Arc::new(
        NirionOciClient::builder()
            .auth(auth)
            .build(),
    );

    let context = NirionContext {
        projects,
        locked_images,
        lock_file,
        oci_client,
        docker_binary: PathBuf::from("docker"),
    };

    handle_command(&cli.command, &context).await?;

    Ok(())
}
