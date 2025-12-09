use anyhow::Result;
use clap::Parser;
use std::{collections::BTreeMap, process::Command as ProcCommand};

use crate::{clap_parse_selector, Project, TargetSelector};

#[derive(Parser, Debug, Clone)]
pub struct LogsArgs {
    #[arg(default_value = "*", value_parser = clap_parse_selector)]
    pub target: TargetSelector,

    /// Execute command in dry run mode
    #[arg(long)]
    pub dry_run: bool,

    /// Follow log output
    #[arg(short = 'f', long)]
    pub follow: bool,

    /// Produce monochrome output
    #[arg(long)]
    pub no_color: bool,

    /// Don't print prefix in logs
    #[arg(long)]
    pub no_log_prefix: bool,

    /// Show logs since timestamp
    #[arg(long)]
    pub since: Option<String>,

    /// Show logs before timestamp
    #[arg(long)]
    pub until: Option<String>,

    /// Number of lines to show from the end
    #[arg(short = 'n', long)]
    pub tail: Option<String>,

    /// Show timestamps
    #[arg(short = 't', long)]
    pub timestamps: bool,
}

fn add_log_flags(args: &mut Vec<String>, logs: &LogsArgs) {
    if logs.follow {
        args.push("--follow".into());
    }
    if logs.no_color {
        args.push("--no-color".into());
    }
    if logs.no_log_prefix {
        args.push("--no-log-prefix".into());
    }
    if logs.timestamps {
        args.push("--timestamps".into());
    }
    if let Some(ref since) = logs.since {
        args.push("--since".into());
        args.push(since.to_string());
    }
    if let Some(ref until) = logs.until {
        args.push("--until".into());
        args.push(until.to_string());
    }
    if let Some(ref tail) = logs.tail {
        args.push("--tail".into());
        args.push(tail.to_string());
    }
}

pub fn handle_logs(
    logs: &LogsArgs,
    projects: &BTreeMap<String, Project>,
) -> Result<()> {
    match &logs.target {
        TargetSelector::All => {
            for (project_name, project) in projects {
                for service_name in project.services.keys() {
                    run_logs(project_name, project, service_name, logs)?;
                }
            }
        }
        TargetSelector::Project(proj) => {
            let project = &projects[&proj.name];
            for service in project.services.keys() {
                run_logs(&proj.name, project, service, logs)?;
            }
        }
        TargetSelector::Image(img) => {
            let project = &projects[&img.project];
            run_logs(&img.project, project, &img.image, logs)?;
        }
    }

    Ok(())
}

fn run_logs(
    project_name: &str,
    project: &Project,
    service_name: &str,
    logs: &LogsArgs,
) -> Result<()> {
    let mut cmd_args = vec![
        "--file".into(),
        project.docker_compose.clone(),
        "--project-name".into(),
        project_name.into(),
        "logs".into(),
    ];

    add_log_flags(&mut cmd_args, logs);
    cmd_args.push(service_name.to_string());

    println!("Running: docker compose {}", cmd_args.join(" "));

    if !logs.dry_run {
        let status = ProcCommand::new("docker")
            .arg("compose")
            .args(&cmd_args)
            .status()?;

        if !status.success() {
            println!(
                "Logs failed for {}.{} (status {})",
                project_name, service_name, status
            );
        }
    }

    Ok(())
}
