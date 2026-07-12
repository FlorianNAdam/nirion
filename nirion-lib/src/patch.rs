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
