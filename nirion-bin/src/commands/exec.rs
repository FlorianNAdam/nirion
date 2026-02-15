use clap::{Parser, ValueHint};
use nirion_lib::{auth::AuthConfig, lock::LockedImages, projects::Projects};
use std::{ops::Deref, path::Path, process::Command as ProcCommand};

use crate::{ClapSelector, ServiceSelector};

/// Execute a command in a running service container
#[derive(Parser, Debug, Clone)]
pub struct ExecArgs {
    /// Service selector: project.service
    #[arg(
        default_value = "*",
        value_parser = ServiceSelector::clap_parse,
        add = ServiceSelector::clap_completer()
    )]
    target: ServiceSelector,

    /// Detached mode: run in background
    #[arg(short = 'd', long)]
    detach: bool,

    /// Disable pseudo-TTY allocation
    #[arg(short = 'T', long)]
    no_tty: bool,

    /// Run as this user
    #[arg(short = 'u', long)]
    user: Option<String>,

    /// Set working directory inside container
    #[arg(short = 'w', long, value_hint = ValueHint::DirPath)]
    workdir: Option<String>,

    /// Container index if service has multiple replicas
    #[arg(long)]
    index: Option<u32>,

    /// Environment variables (can be repeated)
    #[arg(short = 'e', long)]
    env: Vec<String>,

    /// Privileged mode
    #[arg(long)]
    privileged: bool,

    /// Command to execute in container
    cmd: Vec<String>,
}

pub async fn handle_exec(
    args: &ExecArgs,
    projects: &Projects,
    _locked_images: &LockedImages,
    _lock_file: &Path,
    _auth: &AuthConfig,
) -> anyhow::Result<()> {
    if args.cmd.is_empty() {
        anyhow::bail!("No command specified for exec");
    }

    let mut common_args = vec![];
    if args.detach {
        common_args.push("-d".to_string());
    }
    if args.no_tty {
        common_args.push("-T".to_string());
    }
    if let Some(user) = &args.user {
        common_args.push("-u".to_string());
        common_args.push(user.clone());
    }
    if let Some(workdir) = &args.workdir {
        common_args.push("-w".to_string());
        common_args.push(workdir.clone());
    }
    if let Some(idx) = args.index {
        common_args.push("--index".to_string());
        common_args.push(idx.to_string());
    }
    for e in &args.env {
        common_args.push("-e".to_string());
        common_args.push(e.clone());
    }
    if args.privileged {
        common_args.push("--privileged".to_string());
    }

    let project_name = &args.target.project;
    let service_name = &args.target.service;

    let project = &projects[project_name];
    let mut cmd_args = vec![
        "--file".to_string(),
        project.docker_compose.clone(),
        "--project-name".to_string(),
        project.name.deref().to_string(),
        "exec".to_string(),
    ];
    cmd_args.extend(common_args);
    cmd_args.push(service_name.clone());
    cmd_args.extend(args.cmd.clone());

    let status = ProcCommand::new("docker")
        .arg("compose")
        .args(&cmd_args)
        .status()?;

    if !status.success() {
        eprintln!(
            "Command failed in {}.{} with status {}",
            project_name, service_name, status
        );
    }

    Ok(())
}
