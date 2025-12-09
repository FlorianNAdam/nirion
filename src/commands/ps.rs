use anyhow::Result;
use clap::Parser;
use std::{collections::BTreeMap, process::Command as ProcCommand};

use crate::{Project, TargetSelector};

#[derive(Parser, Debug, Clone)]
pub struct PsArgs {
    /// Target selector: *, project, or project.service
    #[arg(default_value = "*", value_parser = crate::clap_parse_selector)]
    pub target: TargetSelector,

    /// Show all containers (including stopped ones)
    #[arg(short = 'a', long)]
    pub all: bool,

    /// Filter services by a property (currently only 'status')
    #[arg(long)]
    pub filter: Option<String>,

    /// Format output (table, json, Go template)
    #[arg(long)]
    pub format: Option<String>,

    /// Short format
    #[arg(long, conflicts_with = "format")]
    pub short: bool,

    /// Don't truncate output
    #[arg(long)]
    pub no_trunc: bool,

    /// Include orphaned services
    #[arg(long)]
    pub orphans: Option<bool>,

    /// Only display container IDs
    #[arg(short = 'q', long)]
    pub quiet: bool,

    /// Display services
    #[arg(long)]
    pub services: bool,

    /// Filter by status (can be repeated)
    #[arg(long)]
    pub status: Vec<String>,
}

pub fn handle_ps(
    args: &PsArgs,
    projects: &BTreeMap<String, Project>,
) -> Result<()> {
    match &args.target {
        TargetSelector::All => {
            for (project_name, project) in projects {
                run_ps(project_name, project, None, args)?;
            }
        }
        TargetSelector::Project(proj) => {
            let project = &projects[&proj.name];
            run_ps(&proj.name, project, None, args)?;
        }
        TargetSelector::Image(img) => {
            let project = &projects[&img.project];
            run_ps(&img.project, project, Some(&img.image), args)?;
        }
    }

    Ok(())
}

fn run_ps(
    project_name: &str,
    project: &Project,
    service: Option<&str>,
    args: &PsArgs,
) -> Result<()> {
    let mut cmd_args = vec![
        "--file".into(),
        project.docker_compose.clone(),
        "--project-name".into(),
        project_name.into(),
        "ps".into(),
    ];

    if args.all {
        cmd_args.push("--all".into());
    }

    if let Some(filter) = &args.filter {
        cmd_args.push("--filter".into());
        cmd_args.push(filter.clone());
    }

    if let Some(format) = &args.format {
        cmd_args.push("--format".into());
        cmd_args.push(format.clone());
    } else if args.short {
        cmd_args.push("--format".into());
        cmd_args.push(
            "table{{.Name}}\t{{.RunningFor}}\t{{.Status}}\t{{.Ports}}"
                .to_string(),
        );
    }

    if args.no_trunc {
        cmd_args.push("--no-trunc".into());
    }

    if let Some(orphans) = args.orphans {
        cmd_args.push(format!("--orphans={orphans}"));
    }

    if args.quiet {
        cmd_args.push("--quiet".into());
    }

    if args.services {
        cmd_args.push("--services".into());
    }

    if !args.status.is_empty() {
        for s in &args.status {
            cmd_args.push("--status".into());
            cmd_args.push(s.clone());
        }
    }

    if let Some(service_name) = service {
        cmd_args.push(service_name.into());
    }

    println!("Running: docker compose {}", cmd_args.join(" "));

    let status = ProcCommand::new("docker")
        .arg("compose")
        .args(&cmd_args)
        .status()?;

    if !status.success() {
        anyhow::bail!(
            "docker compose ps failed for {}{}",
            project_name,
            service.map_or("".to_string(), |s| format!(".{}", s))
        );
    }

    Ok(())
}
