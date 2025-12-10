use anyhow::Result;
use clap::Parser;
use std::{collections::BTreeMap, process::Command as ProcCommand};

use crate::{Project, TargetSelector};

#[derive(Parser, Debug, Clone)]
pub struct VolumesArgs {
    /// Target selector: *, project, or project.service
    #[arg(default_value = "*", value_parser = crate::clap_parse_selector)]
    pub target: TargetSelector,

    /// Output format (table, json, Go template)
    #[arg(long, default_value = "table")]
    pub format: String,

    /// Only display volume names
    #[arg(short = 'q', long)]
    pub quiet: bool,
}

pub fn handle_volumes(
    args: &VolumesArgs,
    projects: &BTreeMap<String, Project>,
) -> Result<()> {
    match &args.target {
        TargetSelector::All => {
            for (project_name, project) in projects {
                run_volumes(project_name, project, None, args)?;
            }
        }
        TargetSelector::Project(proj) => {
            let project = &projects[&proj.name];
            run_volumes(&proj.name, project, None, args)?;
        }
        TargetSelector::Image(img) => {
            let project = &projects[&img.project];
            run_volumes(&img.project, project, Some(&img.image), args)?;
        }
    }

    Ok(())
}

fn run_volumes(
    project_name: &str,
    project: &Project,
    service: Option<&str>,
    args: &VolumesArgs,
) -> Result<()> {
    let mut cmd_args = vec![
        "--file".into(),
        project.docker_compose.clone(),
        "--project-name".into(),
        project_name.into(),
        "volumes".into(),
    ];

    cmd_args.push("--format".into());
    cmd_args.push(args.format.clone());

    if args.quiet {
        cmd_args.push("--quiet".into());
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
            "docker compose volumes failed for {}{}",
            project_name,
            service.map_or("".to_string(), |s| format!(".{}", s))
        );
    }

    Ok(())
}
