use std::{collections::BTreeMap, fs, process::Stdio};

use anyhow::{Context, Result};
use serde::Deserialize;
use tokio::process::Command;

use crate::projects::{Projects, TargetSelector};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchTarget {
    EnvFile,
    Compose,
}

pub async fn patch_target(
    target: &TargetSelector,
    projects: &Projects,
    patch_target: &PatchTarget,
) -> Result<()> {
    match target {
        TargetSelector::All => {
            anyhow::bail!("Only individual projects can be patched");
        }

        TargetSelector::Project(proj) => {
            if patch_target == &PatchTarget::EnvFile {
                anyhow::bail!(
                    "Only individual service env files can be patched"
                );
            }

            let project = &projects[&proj.name];
            patch(&project.docker_compose).await?;
        }

        TargetSelector::Service(img) => {
            let project = &projects[&img.project];

            match patch_target {
                PatchTarget::Compose => {
                    patch(&project.docker_compose).await?;
                }

                PatchTarget::EnvFile => {
                    let compose =
                        load_compose_env_files(&project.docker_compose)?;

                    let service = compose.services.get(&img.service).with_context(|| {
                        format!(
                            "Service `{}` not found in compose file for project `{}`",
                            img.service, img.project
                        )
                    })?;

                    let env_file =
                        service
                            .env_file
                            .first()
                            .with_context(|| {
                                format!(
                                    "No env_file found for `{}.{}`",
                                    img.project, img.service
                                )
                            })?;

                    patch(env_file).await?;
                }
            }
        }
    }

    Ok(())
}

fn load_compose_env_files(path: &str) -> anyhow::Result<ComposeFile> {
    let data = fs::read_to_string(path)
        .with_context(|| format!("Failed reading {}", path))?;

    serde_yaml_ng::from_str::<ComposeFile>(&data)
        .with_context(|| format!("Compose file parse error in {}", path))
}

#[derive(Debug, Deserialize)]
struct ComposeFile {
    services: BTreeMap<String, Service>,
}

#[derive(Debug, Deserialize)]
struct Service {
    #[serde(default)]
    env_file: Vec<String>,
}

pub async fn patch(file: &str) -> Result<()> {
    let status = Command::new("sudo")
        .arg("mirage-patch")
        .arg(file)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .context("failed to spawn mirage-patch")?;

    if !status.success() {
        anyhow::bail!("mirage-patch exited with status: {}", status);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projects(compose_path: &str) -> Projects {
        serde_json::from_value(serde_json::json!({
            "myapp": {
                "name": "myapp",
                "dockerCompose": compose_path,
                "services": {
                    "web": {
                        "image": "nginx",
                        "healthcheck": false,
                        "restart": null
                    }
                }
            }
        }))
        .unwrap()
    }

    fn service_target(service: &str) -> TargetSelector {
        TargetSelector::Service(crate::projects::ServiceSelector {
            project: "myapp".into(),
            service: service.into(),
        })
    }

    #[tokio::test]
    async fn patch_target_rejects_all_projects() {
        let projects = Projects::default();
        let err = patch_target(
            &TargetSelector::All,
            &projects,
            &PatchTarget::Compose,
        )
        .await
        .unwrap_err();

        assert_eq!(err.to_string(), "Only individual projects can be patched");
    }

    #[tokio::test]
    async fn patch_target_rejects_project_env_file() {
        let target =
            TargetSelector::Project(crate::projects::ProjectSelector {
                name: "myapp".into(),
            });
        let projects = projects("compose.yml");
        let err = patch_target(&target, &projects, &PatchTarget::EnvFile)
            .await
            .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Only individual service env files can be patched"
        );
    }

    #[test]
    fn load_compose_env_files_reads_env_files() {
        let dir = tempfile::tempdir().unwrap();
        let compose_path = dir.path().join("compose.yml");
        std::fs::write(
            &compose_path,
            r#"
services:
  web:
    env_file:
      - web.env
      - common.env
  db:
    image: postgres
"#,
        )
        .unwrap();

        let compose =
            load_compose_env_files(compose_path.to_str().unwrap()).unwrap();

        assert_eq!(
            compose.services["web"].env_file,
            vec!["web.env", "common.env"]
        );
        assert!(
            compose.services["db"]
                .env_file
                .is_empty()
        );
    }

    #[test]
    fn load_compose_env_files_reports_missing_file() {
        let err =
            load_compose_env_files("/nonexistent/compose.yml").unwrap_err();

        assert!(
            err.to_string()
                .contains("Failed reading /nonexistent/compose.yml")
        );
    }

    #[test]
    fn load_compose_env_files_reports_invalid_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let compose_path = dir.path().join("compose.yml");
        std::fs::write(&compose_path, "services: [").unwrap();

        let err =
            load_compose_env_files(compose_path.to_str().unwrap()).unwrap_err();

        assert!(
            err.to_string()
                .contains("Compose file parse error")
        );
    }

    #[tokio::test]
    async fn patch_target_reports_missing_service_for_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let compose_path = dir.path().join("compose.yml");
        std::fs::write(
            &compose_path,
            r#"
services:
  web:
    env_file:
      - web.env
"#,
        )
        .unwrap();
        let projects = projects(compose_path.to_str().unwrap());

        let err = patch_target(
            &service_target("worker"),
            &projects,
            &PatchTarget::EnvFile,
        )
        .await
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Service `worker` not found in compose file for project `myapp`"
        );
    }

    #[tokio::test]
    async fn patch_target_reports_missing_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let compose_path = dir.path().join("compose.yml");
        std::fs::write(
            &compose_path,
            r#"
services:
  web:
    image: nginx
"#,
        )
        .unwrap();
        let projects = projects(compose_path.to_str().unwrap());

        let err = patch_target(
            &service_target("web"),
            &projects,
            &PatchTarget::EnvFile,
        )
        .await
        .unwrap_err();

        assert_eq!(err.to_string(), "No env_file found for `myapp.web`");
    }
}
