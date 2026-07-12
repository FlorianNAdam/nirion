use anyhow::{Context, Result};
use serde_json::Value;
use tokio::process::Command;

use crate::{
    docker::query_project_status,
    lock::LockedImages,
    projects::{ProjectSelector, Projects, ServiceSelector},
};

#[cfg(test)]
static TEST_DOCKER_BIN: std::sync::Mutex<Option<String>> =
    std::sync::Mutex::new(None);

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

    let output = docker_command()
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

    let output = docker_command()
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

fn docker_command() -> Command {
    #[cfg(test)]
    if let Some(bin) = TEST_DOCKER_BIN.lock().unwrap().clone() {
        return Command::new(bin);
    }

    Command::new("docker")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::VersionedImage;
    use std::{fs, os::unix::fs::PermissionsExt, path::Path};

    static DOCKER_BIN_LOCK: tokio::sync::Mutex<()> =
        tokio::sync::Mutex::const_new(());

    struct DockerBinGuard;

    impl DockerBinGuard {
        fn set(bin: String) -> Self {
            *TEST_DOCKER_BIN.lock().unwrap() = Some(bin);
            Self
        }
    }

    impl Drop for DockerBinGuard {
        fn drop(&mut self) {
            *TEST_DOCKER_BIN.lock().unwrap() = None;
        }
    }

    fn projects() -> Projects {
        serde_json::from_value(serde_json::json!({
            "myapp": {
                "name": "myapp",
                "dockerCompose": "compose.yml",
                "services": {
                    "web": {
                        "image": "nginx:latest",
                        "healthcheck": false,
                        "restart": null
                    },
                    "worker": {
                        "image": null,
                        "healthcheck": false,
                        "restart": null
                    }
                }
            }
        }))
        .unwrap()
    }

    fn target(service: &str) -> ServiceSelector {
        ServiceSelector {
            project: "myapp".into(),
            service: service.into(),
        }
    }

    fn write_fake_docker(
        dir: &Path,
        args_file: &Path,
        stdout: &str,
        stderr: &str,
        exit_code: i32,
    ) -> String {
        let docker = dir.join("docker");
        fs::write(
            &docker,
            format!(
                r#"#!/bin/sh
printf '%s\n' "$@" > '{}'
printf '%s\n' '{}'
printf '%s\n' '{}' >&2
exit {exit_code}
"#,
                args_file.display(),
                stdout,
                stderr,
            ),
        )
        .unwrap();

        let mut permissions = fs::metadata(&docker)
            .unwrap()
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&docker, permissions).unwrap();

        docker.to_string_lossy().to_string()
    }

    #[test]
    fn pretty_json_formats_object_json() {
        let rendered = pretty_json(r#"{"image":"nginx","ports":[80,443]}"#);

        assert_eq!(
            rendered,
            r#"{
  "image": "nginx",
  "ports": [
    80,
    443
  ]
}"#
        );
    }

    #[test]
    fn pretty_json_formats_array_json() {
        let rendered = pretty_json(r#"[{"name":"web"},{"name":"db"}]"#);

        assert_eq!(
            rendered,
            r#"[
  {
    "name": "web"
  },
  {
    "name": "db"
  }
]"#
        );
    }

    #[test]
    fn pretty_json_returns_invalid_json_unchanged() {
        let output = "not json";

        assert_eq!(pretty_json(output), output);
    }

    #[tokio::test]
    async fn inspect_image_uses_base_image_without_lock_entry() {
        let _docker_bin_lock = DOCKER_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(
            dir.path(),
            &args_file,
            r#"{"Id":"image-id"}"#,
            "",
            0,
        );
        let _docker_bin_guard = DockerBinGuard::set(docker);

        let output = inspect_image(
            &target("web"),
            &projects(),
            &LockedImages::default(),
            "{{json .}}",
            true,
        )
        .await
        .unwrap();

        assert_eq!(output, r#"{"Id":"image-id"}"#.to_string() + "\n");
        assert_eq!(
            fs::read_to_string(args_file).unwrap(),
            "image\ninspect\n--format\n{{json .}}\nnginx:latest\n"
        );
    }

    #[tokio::test]
    async fn inspect_image_uses_locked_digest_and_pretty_prints_json() {
        let _docker_bin_lock = DOCKER_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(
            dir.path(),
            &args_file,
            r#"{"RepoTags":["nginx:latest"]}"#,
            "",
            0,
        );
        let _docker_bin_guard = DockerBinGuard::set(docker);
        let mut locked_images = LockedImages::default();
        locked_images.insert(
            "myapp.web".into(),
            VersionedImage {
                image: "nginx:latest".into(),
                version: Some("1.27.0".into()),
                digest: "sha256:abc".into(),
            },
        );

        let output = inspect_image(
            &target("web"),
            &projects(),
            &locked_images,
            "{{json .}}",
            false,
        )
        .await
        .unwrap();

        assert_eq!(
            output,
            r#"{
  "RepoTags": [
    "nginx:latest"
  ]
}"#
        );
        assert_eq!(
            fs::read_to_string(args_file).unwrap(),
            "image\ninspect\n--format\n{{json .}}\nnginx:latest@sha256:abc\n"
        );
    }

    #[tokio::test]
    async fn inspect_image_reports_missing_service_image_before_running_docker()
    {
        let _docker_bin_lock = DOCKER_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, "{}", "", 0);
        let _docker_bin_guard = DockerBinGuard::set(docker);

        let err = inspect_image(
            &target("worker"),
            &projects(),
            &LockedImages::default(),
            "{{json .}}",
            true,
        )
        .await
        .unwrap_err();

        assert_eq!(err.to_string(), "Image missing from service worker");
        assert!(!args_file.exists());
    }

    #[tokio::test]
    async fn inspect_image_reports_docker_failure_with_stderr() {
        let _docker_bin_lock = DOCKER_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(
            dir.path(),
            &args_file,
            "",
            "image not found",
            17,
        );
        let _docker_bin_guard = DockerBinGuard::set(docker);

        let err = inspect_image(
            &target("web"),
            &projects(),
            &LockedImages::default(),
            "{{json .}}",
            true,
        )
        .await
        .unwrap_err();

        let err = err.to_string();
        assert!(err.contains("docker image inspect failed with status"));
        assert!(err.contains("image not found"));
    }

    #[tokio::test]
    async fn inspect_service_dispatches_image_inspect() {
        let _docker_bin_lock = DOCKER_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker =
            write_fake_docker(dir.path(), &args_file, r#"{"Id":"abc"}"#, "", 0);
        let _docker_bin_guard = DockerBinGuard::set(docker);

        let output = inspect_service(
            &target("web"),
            &InspectTarget::Image,
            &projects(),
            &LockedImages::default(),
            "{{json .}}",
            false,
        )
        .await
        .unwrap();

        assert_eq!(
            output,
            r#"{
  "Id": "abc"
}"#
        );
    }

    #[tokio::test]
    async fn inspect_project_collects_outputs_for_services() {
        let _docker_bin_lock = DOCKER_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker =
            write_fake_docker(dir.path(), &args_file, r#"{"ok":true}"#, "", 0);
        let _docker_bin_guard = DockerBinGuard::set(docker);
        let projects: Projects = serde_json::from_value(serde_json::json!({
            "myapp": {
                "name": "myapp",
                "dockerCompose": "compose.yml",
                "services": {
                    "web": {
                        "image": "nginx:latest",
                        "healthcheck": false,
                        "restart": null
                    }
                }
            }
        }))
        .unwrap();

        let outputs = inspect_project(
            &crate::projects::ProjectSelector {
                name: "myapp".into(),
            },
            &InspectTarget::Image,
            &projects,
            &LockedImages::default(),
            "{{json .}}",
            true,
        )
        .await
        .unwrap();

        assert_eq!(outputs, vec![r#"{"ok":true}"#.to_string() + "\n"]);
    }

    #[tokio::test]
    async fn inspect_project_reports_service_failures() {
        let _docker_bin_lock = DOCKER_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, "{}", "", 0);
        let _docker_bin_guard = DockerBinGuard::set(docker);

        let err = inspect_project(
            &crate::projects::ProjectSelector {
                name: "myapp".into(),
            },
            &InspectTarget::Image,
            &projects(),
            &LockedImages::default(),
            "{{json .}}",
            true,
        )
        .await
        .unwrap_err();

        let err = err.to_string();
        assert!(err.contains("failed to inspect 1 service(s)"));
        assert!(
            err.contains("myapp.worker: Image missing from service worker")
        );
    }
}
