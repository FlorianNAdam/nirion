use anyhow::{Context, Result};
use serde_json::Value;

use crate::{
    context::NirionContext,
    docker::query_project_status,
    projects::{ProjectSelector, ServiceSelector},
};

pub async fn inspect_project_images(
    context: &NirionContext,
    target: &ProjectSelector,
    format: &str,
    raw: bool,
) -> anyhow::Result<Vec<String>> {
    let mut failures = Vec::new();
    let mut outputs = Vec::new();

    for service in context.projects[&target.name]
        .services
        .keys()
    {
        let service_selector = ServiceSelector {
            project: target.name.to_string(),
            service: service.to_string(),
        };
        match inspect_image(context, &service_selector, format, raw).await {
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

pub async fn inspect_project_containers(
    context: &NirionContext,
    target: &ProjectSelector,
    format: &str,
    raw: bool,
) -> anyhow::Result<Vec<String>> {
    let mut failures = Vec::new();
    let mut outputs = Vec::new();

    for service in context.projects[&target.name]
        .services
        .keys()
    {
        let service_selector = ServiceSelector {
            project: target.name.to_string(),
            service: service.to_string(),
        };
        match inspect_container(context, &service_selector, format, raw).await {
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

pub async fn inspect_image(
    context: &NirionContext,
    target: &ServiceSelector,
    format: &str,
    raw: bool,
) -> Result<String> {
    let project = &context.projects[&target.project];

    let service = project
        .services
        .get(&target.service)
        .ok_or_else(|| {
            anyhow::anyhow!("Service {} missing from project", &target.service)
        })?;

    let image_name = service
        .resolved_image
        .clone()
        .ok_or_else(|| {
            anyhow::anyhow!("Image missing from service {}", &target.service)
        })?;

    let output = context
        .docker_command
        .command()
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

pub async fn inspect_container(
    context: &NirionContext,
    target: &ServiceSelector,
    format: &str,
    raw: bool,
) -> Result<String> {
    let project_status = query_project_status(context, &target.project).await?;

    let service_status = project_status
        .services
        .get(&target.service)
        .ok_or_else(|| {
            anyhow::anyhow!("Service {} missing from status", &target.service)
        })?;

    let output = context
        .docker_command
        .command()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        docker::DockerCommand,
        lock::{LockedImages, VersionedImage},
        projects::Projects,
    };
    use nirion_oci_lib::client::NirionOciClient;
    use std::{
        fs,
        io::Write,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        sync::Arc,
    };

    fn projects() -> Projects {
        serde_json::from_value(serde_json::json!({
            "myapp": {
                "name": "myapp",
                "dockerCompose": "compose.yml",
                "services": {
                    "web": {
                        "image": "nginx:latest",
                        "resolvedImage": "nginx:latest@sha256:evaluated-generation",
                        "healthcheck": false,
                        "restart": null
                    },
                    "worker": {
                        "image": null,
                        "resolvedImage": null,
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

    fn context(
        docker_command: DockerCommand,
        projects: Projects,
        locked_images: LockedImages,
    ) -> NirionContext {
        NirionContext {
            projects,
            locked_images,
            lock_file: PathBuf::from("lock.json"),
            oci_client: Arc::new(NirionOciClient::builder().build()),
            docker_command,
        }
    }

    fn write_fake_docker(
        dir: &Path,
        args_file: &Path,
        stdout: &str,
        stderr: &str,
        exit_code: i32,
    ) -> String {
        let docker = dir.join("docker-image");
        let tmp = dir.join("docker-image.tmp");
        let mut file = fs::File::create(&tmp).unwrap();
        file.write_all(
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
            )
            .as_bytes(),
        )
        .unwrap();
        file.sync_all().unwrap();
        drop(file);

        let mut permissions = fs::metadata(&tmp)
            .unwrap()
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&tmp, permissions).unwrap();
        fs::rename(&tmp, &docker).unwrap();

        docker.to_string_lossy().to_string()
    }

    fn write_fake_container_docker(
        dir: &Path,
        args_file: &Path,
        compose_stdout: &str,
        inspect_stdout: &str,
        inspect_stderr: &str,
        inspect_exit_code: i32,
    ) -> String {
        let docker = dir.join("docker-container");
        let tmp = dir.join("docker-container.tmp");
        let mut file = fs::File::create(&tmp).unwrap();
        file.write_all(
            format!(
                r#"#!/bin/sh
printf '%s\n' "$@" >> '{}'
if [ "$1" = "compose" ]; then
  printf '%s\n' '{}'
  exit 0
fi
printf '%s\n' '{}'
printf '%s\n' '{}' >&2
exit {inspect_exit_code}
"#,
                args_file.display(),
                compose_stdout,
                inspect_stdout,
                inspect_stderr,
            )
            .as_bytes(),
        )
        .unwrap();
        file.sync_all().unwrap();
        drop(file);

        let mut permissions = fs::metadata(&tmp)
            .unwrap()
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&tmp, permissions).unwrap();
        fs::rename(&tmp, &docker).unwrap();

        docker.to_string_lossy().to_string()
    }

    fn fake_docker_command(script: &str) -> DockerCommand {
        DockerCommand::with_args("/bin/sh", [script])
    }

    fn compose_ps_service(
        service: &str,
        id: &str,
    ) -> String {
        serde_json::json!({
            "ID": id,
            "Name": format!("myapp-{service}-1"),
            "Service": service,
            "Image": "nginx:latest",
            "State": "running",
            "Health": null,
            "ExitCode": null,
            "RunningFor": "1 minute",
            "Status": "Up 1 minute",
            "Ports": "",
            "Networks": "default"
        })
        .to_string()
    }

    fn assert_json_eq(
        actual: &str,
        expected: Value,
    ) {
        assert_eq!(serde_json::from_str::<Value>(actual).unwrap(), expected);
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
    async fn inspect_image_uses_resolved_image() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(
            dir.path(),
            &args_file,
            r#"{"Id":"image-id"}"#,
            "",
            0,
        );

        let output = inspect_image(
            &context(
                fake_docker_command(&docker),
                projects(),
                LockedImages::default(),
            ),
            &target("web"),
            "{{json .}}",
            true,
        )
        .await
        .unwrap();

        assert_json_eq(&output, serde_json::json!({ "Id": "image-id" }));
        assert_eq!(
            fs::read_to_string(args_file).unwrap(),
            "image\ninspect\n--format\n{{json .}}\nnginx:latest@sha256:evaluated-generation\n"
        );
    }

    #[tokio::test]
    async fn inspect_image_pretty_prints_json() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(
            dir.path(),
            &args_file,
            r#"{"RepoTags":["nginx:latest"]}"#,
            "",
            0,
        );
        let output = inspect_image(
            &context(
                fake_docker_command(&docker),
                projects(),
                LockedImages::default(),
            ),
            &target("web"),
            "{{json .}}",
            false,
        )
        .await
        .unwrap();

        assert_json_eq(
            &output,
            serde_json::json!({ "RepoTags": ["nginx:latest"] }),
        );
        assert_eq!(
            fs::read_to_string(args_file).unwrap(),
            "image\ninspect\n--format\n{{json .}}\nnginx:latest@sha256:evaluated-generation\n"
        );
    }

    #[tokio::test]
    async fn inspect_image_prefers_resolved_image_over_lock_file() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(
            dir.path(),
            &args_file,
            r#"{"RepoTags":["nginx:latest"]}"#,
            "",
            0,
        );
        let mut locked_images = LockedImages::default();
        locked_images.insert(
            "myapp.web".into(),
            VersionedImage {
                image: "nginx:latest".into(),
                version: Some("1.28.0".into()),
                digest: "sha256:current-lock".into(),
            },
        );
        let projects: Projects = serde_json::from_value(serde_json::json!({
            "myapp": {
                "name": "myapp",
                "dockerCompose": "compose.yml",
                "services": {
                    "web": {
                        "image": "nginx:latest",
                        "resolvedImage": "nginx:latest@sha256:evaluated-generation",
                        "healthcheck": false,
                        "restart": null
                    }
                }
            }
        }))
        .unwrap();

        let output = inspect_image(
            &context(fake_docker_command(&docker), projects, locked_images),
            &target("web"),
            "{{json .}}",
            false,
        )
        .await
        .unwrap();

        assert_json_eq(
            &output,
            serde_json::json!({ "RepoTags": ["nginx:latest"] }),
        );
        assert_eq!(
            fs::read_to_string(args_file).unwrap(),
            "image\ninspect\n--format\n{{json .}}\nnginx:latest@sha256:evaluated-generation\n"
        );
    }

    #[tokio::test]
    async fn inspect_image_reports_missing_service_image_before_running_docker()
    {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, "{}", "", 0);

        let err = inspect_image(
            &context(
                fake_docker_command(&docker),
                projects(),
                LockedImages::default(),
            ),
            &target("worker"),
            "{{json .}}",
            true,
        )
        .await
        .unwrap_err();

        assert_eq!(err.to_string(), "Image missing from service worker");
        assert!(!args_file.exists());
    }

    #[tokio::test]
    async fn inspect_image_reports_missing_service_before_running_docker() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, "{}", "", 0);

        let err = inspect_image(
            &context(
                fake_docker_command(&docker),
                projects(),
                LockedImages::default(),
            ),
            &target("missing"),
            "{{json .}}",
            true,
        )
        .await
        .unwrap_err();

        assert_eq!(err.to_string(), "Service missing missing from project");
        assert!(!args_file.exists());
    }

    #[tokio::test]
    async fn inspect_image_reports_docker_failure_with_stderr() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(
            dir.path(),
            &args_file,
            "",
            "image not found",
            17,
        );

        let err = inspect_image(
            &context(
                fake_docker_command(&docker),
                projects(),
                LockedImages::default(),
            ),
            &target("web"),
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
    async fn inspect_container_pretty_prints_container_json() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_container_docker(
            dir.path(),
            &args_file,
            &compose_ps_service("web", "container-123"),
            r#"{"Name":"myapp-web-1"}"#,
            "",
            0,
        );

        let output = inspect_container(
            &context(
                fake_docker_command(&docker),
                projects(),
                LockedImages::default(),
            ),
            &target("web"),
            "{{json .}}",
            false,
        )
        .await
        .unwrap();

        assert_json_eq(&output, serde_json::json!({ "Name": "myapp-web-1" }));
        assert!(
            fs::read_to_string(args_file)
                .unwrap()
                .contains("inspect\n--format\n{{json .}}\ncontainer-123\n")
        );
    }

    #[tokio::test]
    async fn inspect_container_reports_missing_service_status() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_container_docker(
            dir.path(),
            &args_file,
            &compose_ps_service("db", "container-db"),
            r#"{}"#,
            "",
            0,
        );

        let err = inspect_container(
            &context(
                fake_docker_command(&docker),
                projects(),
                LockedImages::default(),
            ),
            &target("web"),
            "{{json .}}",
            true,
        )
        .await
        .unwrap_err();

        assert_eq!(err.to_string(), "Service web missing from status");
    }

    #[tokio::test]
    async fn inspect_container_reports_docker_failure_with_stderr() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_container_docker(
            dir.path(),
            &args_file,
            &compose_ps_service("web", "container-123"),
            "",
            "container gone",
            4,
        );

        let err = inspect_container(
            &context(
                fake_docker_command(&docker),
                projects(),
                LockedImages::default(),
            ),
            &target("web"),
            "{{json .}}",
            true,
        )
        .await
        .unwrap_err();

        let err = err.to_string();
        assert!(err.contains("docker inspect failed with status"));
        assert!(err.contains("container gone"));
    }

    #[tokio::test]
    async fn inspect_project_images_collects_outputs_for_services() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker =
            write_fake_docker(dir.path(), &args_file, r#"{"ok":true}"#, "", 0);
        let projects: Projects = serde_json::from_value(serde_json::json!({
            "myapp": {
                "name": "myapp",
                "dockerCompose": "compose.yml",
                "services": {
                    "web": {
                        "image": "nginx:latest",
                        "resolvedImage": "nginx:latest@sha256:evaluated-generation",
                        "healthcheck": false,
                        "restart": null
                    }
                }
            }
        }))
        .unwrap();

        let outputs = inspect_project_images(
            &context(
                fake_docker_command(&docker),
                projects,
                LockedImages::default(),
            ),
            &crate::projects::ProjectSelector {
                name: "myapp".into(),
            },
            "{{json .}}",
            true,
        )
        .await
        .unwrap();

        assert_eq!(outputs.len(), 1);
        assert_json_eq(&outputs[0], serde_json::json!({ "ok": true }));
    }

    #[tokio::test]
    async fn inspect_project_images_reports_service_failures() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, "{}", "", 0);

        let err = inspect_project_images(
            &context(
                fake_docker_command(&docker),
                projects(),
                LockedImages::default(),
            ),
            &crate::projects::ProjectSelector {
                name: "myapp".into(),
            },
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
