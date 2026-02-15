use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use nirion_lib::{
    auth::AuthConfig,
    lock::LockedImages,
    projects::{ProjectSelector, Projects, ServiceSelector, TargetSelector},
};
use serde_json::Value;
use std::path::Path;
use tokio::process::Command;

use crate::{docker::query_project_status, ClapSelector};

/// Patch service files using mirage-patch
#[derive(Parser, Debug, Clone)]
pub struct InspectArgs {
    /// Target selector: *, project, or project.service
    #[arg(
        default_value = "*",
        value_parser = TargetSelector::clap_parse,
        add = TargetSelector::clap_completer()
    )]
    pub target: TargetSelector,

    /// What to inspect
    #[arg(short, long, value_enum, default_value = "container")]
    inspect_target: InspectTarget,

    /// The inspect format
    #[arg(short, long, default_value = "json")]
    format: String,

    /// Print json without pretty printing
    #[arg(short, long)]
    raw: bool,
}

#[derive(Clone, Debug, ValueEnum, PartialEq, Eq)]
enum InspectTarget {
    Image,
    Container,
}

pub async fn handle_inspect(
    args: &InspectArgs,
    projects: &Projects,
    locked_images: &LockedImages,
    _lock_file: &Path,
    _auth: &AuthConfig,
) -> Result<()> {
    match &args.target {
        TargetSelector::All => {
            for (project_name, _) in projects.iter() {
                let project_selector = ProjectSelector {
                    name: project_name.to_string(),
                };
                inspect_project(
                    &project_selector,
                    &args.inspect_target,
                    projects,
                    locked_images,
                    &args.format,
                    args.raw,
                )
                .await?
            }
        }
        TargetSelector::Project(proj) => {
            inspect_project(
                &proj,
                &args.inspect_target,
                projects,
                locked_images,
                &args.format,
                args.raw,
            )
            .await?
        }
        TargetSelector::Service(img) => {
            inspect_service(
                &img,
                &args.inspect_target,
                projects,
                locked_images,
                &args.format,
                args.raw,
            )
            .await?
        }
    }
    Ok(())
}

async fn inspect_project(
    target: &ProjectSelector,
    inspect_target: &InspectTarget,
    projects: &Projects,
    locked_images: &LockedImages,
    format: &str,
    raw: bool,
) -> anyhow::Result<()> {
    for service in projects[&target.name].services.keys() {
        let service_selector = ServiceSelector {
            project: target.name.to_string(),
            service: service.to_string(),
        };
        if let Err(e) = inspect_service(
            &service_selector,
            inspect_target,
            projects,
            locked_images,
            format,
            raw,
        )
        .await
        {
            eprintln!(
                "Failed to inspect service {}.{}:{}",
                target.name, service, e
            );
        };
    }

    Ok(())
}

async fn inspect_service(
    target: &ServiceSelector,
    inspect_target: &InspectTarget,
    projects: &Projects,
    locked_images: &LockedImages,
    format: &str,
    raw: bool,
) -> anyhow::Result<()> {
    match inspect_target {
        InspectTarget::Image => {
            inspect_image(target, projects, locked_images, format, raw).await?
        }
        InspectTarget::Container => {
            inspect_container(&target, projects, &format, raw).await?
        }
    }
    Ok(())
}

async fn inspect_image(
    target: &ServiceSelector,
    projects: &Projects,
    locked_images: &LockedImages,
    format: &str,
    raw: bool,
) -> Result<()> {
    let project = &projects[&target.project];

    let service = project
        .services
        .get(&target.service)
        .ok_or_else(|| {
            anyhow::anyhow!("Service {} missing from project", &target.service)
        })?;

    let base_image = service.image.as_ref().ok_or_else(|| {
        anyhow::anyhow!("Image missing from service {}", &target.service)
    })?;

    let identifier = format!("{}.{}", target.project, target.service);

    let image_name = if let Some(digest) = locked_images.get(&identifier) {
        format!("{}@{}", base_image, digest.digest)
    } else {
        base_image.to_string()
    };

    let output = Command::new("docker")
        .arg("image")
        .arg("inspect")
        .arg("--format")
        .arg(format)
        .arg(&image_name)
        .output()
        .await
        .context("failed to execute docker image inspect")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("{}", stderr);
        anyhow::bail!(
            "docker image inspect failed with status {}",
            output.status
        );
    }

    let output = str::from_utf8(&output.stdout)?.to_string();

    if !raw {
        pretty_print_json(&output);
    } else {
        println!("{output}");
    }

    Ok(())
}

async fn inspect_container(
    target: &ServiceSelector,
    projects: &Projects,
    format: &str,
    raw: bool,
) -> Result<()> {
    let project = &projects.get(&target.project).unwrap();

    let project_status =
        query_project_status(&project.docker_compose, &project.name).await?;

    let service_status = project_status
        .services
        .get(&target.service)
        .ok_or_else(|| {
            anyhow::anyhow!("Service {} missing from status", &target.service)
        })?;

    let output = Command::new("docker")
        .arg("inspect")
        .arg("--format")
        .arg(format)
        .arg(&service_status.id)
        .output()
        .await
        .context("failed to execute docker inspect")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("{}", stderr);
        anyhow::bail!("docker inspect failed with status {}", output.status);
    }

    let output = str::from_utf8(&output.stdout)?.to_string();

    if !raw {
        pretty_print_json(&output);
    } else {
        println!("{output}");
    }

    Ok(())
}

fn pretty_print_json(string: &str) {
    fn inner(string: &str) -> anyhow::Result<String> {
        let json = serde_json::from_str::<Value>(&string)?;
        let raw = serde_json::to_string_pretty(&json)?;
        Ok(raw)
    }

    match inner(string) {
        Ok(raw) => println!("{raw}"),
        Err(_) => println!("{string}"),
    }
}
