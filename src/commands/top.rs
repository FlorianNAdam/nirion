use anyhow::Result;
use clap::Parser;
use std::{collections::BTreeMap, process::Command as ProcCommand};

use crate::{Project, TargetSelector};

#[derive(Parser, Debug, Clone)]
pub struct TopArgs {
    /// Target selector: *, project, or project.service
    #[arg(default_value = "*", value_parser = crate::clap_parse_selector)]
    pub target: TargetSelector,
}

pub fn handle_top(
    args: &TopArgs,
    projects: &BTreeMap<String, Project>,
) -> Result<()> {
    match &args.target {
        TargetSelector::All => {
            for (project_name, project) in projects {
                run_top(project_name, project, None)?;
            }
        }
        TargetSelector::Project(proj) => {
            let project = &projects[&proj.name];
            run_top(&proj.name, project, None)?;
        }
        TargetSelector::Image(img) => {
            let project = &projects[&img.project];
            run_top(&img.project, project, Some(&img.image))?;
        }
    }

    Ok(())
}

fn run_top(
    project_name: &str,
    project: &Project,
    service: Option<&str>,
) -> Result<()> {
    let mut cmd_args = vec![
        "--file".into(),
        project.docker_compose.clone(),
        "--project-name".into(),
        project_name.into(),
        "top".into(),
    ];

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
            "docker compose top failed for {}{}",
            project_name,
            service.map_or("".to_string(), |s| format!(".{}", s))
        );
    }

    Ok(())
}
