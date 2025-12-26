use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use tokio::process::Command;

use crate::{
    clap_parse_service_selector, docker::query_project_status, Project,
    ServiceSelector,
};

/// Patch service files using mirage-patch
#[derive(Parser, Debug, Clone)]
pub struct InspectArgs {
    /// Service selector: project.service
    #[arg(value_parser = clap_parse_service_selector)]
    target: ServiceSelector,

    /// What to patch
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
    projects: &BTreeMap<String, Project>,
    locked_images: &BTreeMap<String, String>,
    _lock_file: &Path,
) -> Result<()> {
    match args.inspect_target {
        InspectTarget::Image => {
            inspect_image(
                &args.target,
                projects,
                locked_images,
                &args.format,
                args.raw,
            )
            .await?
        }
        InspectTarget::Container => {
            inspect_container(&args.target, projects, &args.format, args.raw)
                .await?
        }
    }

    Ok(())
}

pub async fn inspect_image(
    target: &ServiceSelector,
    projects: &BTreeMap<String, Project>,
    locked_images: &BTreeMap<String, String>,
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

    let digest = locked_images
        .get(&identifier)
        .ok_or_else(|| {
            anyhow::anyhow!("Image missing from lock file {}", &target.service)
        })?;

    let image_name = format!("{}@{}", base_image, digest);

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

pub async fn inspect_container(
    target: &ServiceSelector,
    projects: &BTreeMap<String, Project>,
    format: &str,
    raw: bool,
) -> Result<()> {
    let project = &projects[&target.project];

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
