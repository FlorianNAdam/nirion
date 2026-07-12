use anyhow::{Context, Result};
use serde_json::Value;
use tokio::process::Command;

use crate::{
    docker::query_project_status,
    lock::LockedImages,
    projects::{ProjectSelector, Projects, ServiceSelector},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InspectTarget {
    Image,
    Container,
}

pub async fn inspect_project(
    target: &ProjectSelector,
    inspect_target: &InspectTarget,
    projects: &Projects,
    locked_images: &LockedImages,
    format: &str,
    raw: bool,
) -> anyhow::Result<Vec<String>> {
    let mut failures = Vec::new();
    let mut outputs = Vec::new();

    for service in projects[&target.name].services.keys() {
        let service_selector = ServiceSelector {
            project: target.name.to_string(),
            service: service.to_string(),
        };
        match inspect_service(
            &service_selector,
            inspect_target,
            projects,
            locked_images,
            format,
            raw,
        )
        .await
        {
            Ok(output) => outputs.push(output),
            Err(e) => {
                failures.push(format!("{}.{}: {}", target.name, service, e))
            }
        }
    }

    if !failures.is_empty() {
        anyhow::bail!(
            "failed to inspect {} service(s): {}",
            failures.len(),
            failures.join("; ")
        );
    }

    Ok(outputs)
}

pub async fn inspect_service(
    target: &ServiceSelector,
    inspect_target: &InspectTarget,
    projects: &Projects,
    locked_images: &LockedImages,
    format: &str,
    raw: bool,
) -> anyhow::Result<String> {
    let output = match inspect_target {
        InspectTarget::Image => {
            inspect_image(target, projects, locked_images, format, raw).await?
        }
        InspectTarget::Container => {
            inspect_container(target, projects, format, raw).await?
        }
    };
    Ok(output)
}

async fn inspect_image(
    target: &ServiceSelector,
    projects: &Projects,
    locked_images: &LockedImages,
    format: &str,
    raw: bool,
) -> Result<String> {
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
        anyhow::bail!(
            "docker image inspect failed with status {}{}{}",
            output.status,
            if stderr.trim().is_empty() { "" } else { ": " },
            stderr.trim()
        );
    }

    let output = str::from_utf8(&output.stdout)?.to_string();

    if raw {
        Ok(output)
    } else {
        Ok(pretty_json(&output))
    }
}

async fn inspect_container(
    target: &ServiceSelector,
    projects: &Projects,
    format: &str,
    raw: bool,
) -> Result<String> {
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
        anyhow::bail!(
            "docker inspect failed with status {}{}{}",
            output.status,
            if stderr.trim().is_empty() { "" } else { ": " },
            stderr.trim()
        );
    }

    let output = str::from_utf8(&output.stdout)?.to_string();

    if raw {
        Ok(output)
    } else {
        Ok(pretty_json(&output))
    }
}

fn pretty_json(string: &str) -> String {
    fn inner(string: &str) -> anyhow::Result<String> {
        let json = serde_json::from_str::<Value>(string)?;
        let raw = serde_json::to_string_pretty(&json)?;
        Ok(raw)
    }

    match inner(string) {
        Ok(raw) => raw,
        Err(_) => string.to_string(),
    }
}
