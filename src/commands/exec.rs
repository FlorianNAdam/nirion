use anyhow::Result;
use clap::{Parser, ValueHint};
use std::{collections::BTreeMap, process::Command as ProcCommand};

use crate::{clap_parse_image_selector, ImageSelector, Project};

#[derive(Parser, Debug, Clone)]
pub struct ExecArgs {
    #[arg(value_parser = clap_parse_image_selector)]
    target: ImageSelector,

    /// Detached mode: run in background
    #[arg(short = 'd', long)]
    detach: bool,

    /// Execute command in dry run mode
    #[arg(long)]
    dry_run: bool,

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

pub fn handle_exec(
    args: &ExecArgs,
    projects: &BTreeMap<String, Project>,
) -> Result<()> {
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
    let service_name = &args.target.image;

    let project = &projects[project_name];
    let mut cmd_args = vec![
        "--file".to_string(),
        project.docker_compose.clone(),
        "--project-name".to_string(),
        project_name.clone(),
        "exec".to_string(),
    ];
    cmd_args.extend(common_args);
    cmd_args.push(service_name.clone());
    cmd_args.extend(args.cmd.clone());

    println!("Running: docker compose {:?}", cmd_args.join(" "));

    if !args.dry_run {
        let status = ProcCommand::new("docker")
            .arg("compose")
            .args(&cmd_args)
            .status()?;

        if status.success() {
            println!(
                "Command executed successfully in {}.{}",
                project_name, service_name
            );
        } else {
            println!(
                "Command failed in {}.{} with status {}",
                project_name, service_name, status
            );
        }
    }

    Ok(())
}
