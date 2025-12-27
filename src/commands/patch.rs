use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use serde::Deserialize;
use serde_yml as serde_yaml;
use std::path::Path;
use std::process::Stdio;
use std::{collections::BTreeMap, fs};
use tokio::process::Command;

use crate::{Project, TargetSelector};

/// Patch service files using mirage-patch
#[derive(Parser, Debug, Clone)]
pub struct PatchArgs {
    /// Target selector: *, project, or project.service
    #[arg(
        default_value = "*",
        value_parser = TargetSelector::clap_parse,
        add = TargetSelector::clap_completer()
    )]
    pub target: TargetSelector,

    /// What to patch
    #[arg(short, long, value_enum, default_value = "compose")]
    patch_target: PatchTarget,
}

#[derive(Clone, Debug, ValueEnum, PartialEq, Eq)]
enum PatchTarget {
    EnvFile,
    Compose,
}

pub async fn handle_patch(
    args: &PatchArgs,
    projects: &BTreeMap<String, Project>,
    _locked_images: &BTreeMap<String, String>,
    _lock_file: &Path,
) -> Result<()> {
    match &args.target {
        TargetSelector::All => {
            anyhow::bail!("Only individual projects can be patched");
        }

        TargetSelector::Project(proj) => {
            if args.patch_target == PatchTarget::EnvFile {
                anyhow::bail!(
                    "Only individual service env files can be patched"
                );
            }

            let project = &projects[&proj.name];
            patch(&project.docker_compose).await?;
        }

        TargetSelector::Service(img) => {
            let project = &projects[&img.project];

            match args.patch_target {
                PatchTarget::Compose => {
                    patch(&project.docker_compose).await?;
                }

                PatchTarget::EnvFile => {
                    let compose = load_compose(&project.docker_compose)?;

                    let service = compose
                        .services
                        .get(&img.service)
                        .with_context(|| {
                            format!(
                                "Service `{}` not found in docker-compose for project `{}`",
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

fn load_compose(path: &str) -> anyhow::Result<ComposeFile> {
    let data = fs::read_to_string(path)
        .with_context(|| format!("Failed reading {}", path))?;

    serde_yaml::from_str::<ComposeFile>(&data)
        .with_context(|| format!("YAML parse error in {}", path))
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
