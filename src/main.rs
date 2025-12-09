use anyhow::Result;
use clap::{Parser, Subcommand};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap, fs, path::PathBuf, process::Command as ProcCommand,
};

use crate::{
    down::handle_down,
    exec::{handle_exec, ExecArgs},
    lock::handle_lock,
    logs::{handle_logs, LogsArgs},
    up::handle_up,
    update::handle_update,
};

mod down;
mod exec;
mod lock;
mod logs;
mod up;
mod update;

#[derive(Clone, Debug)]
struct ProjectSelector {
    name: String,
}

#[derive(Clone, Debug)]
struct ImageSelector {
    project: String,
    image: String,
}

#[derive(Clone, Debug)]
enum TargetSelector {
    All,
    Project(ProjectSelector),
    Image(ImageSelector),
}

#[derive(Debug, Serialize, Deserialize)]
struct Project {
    #[serde(rename = "docker-compose")]
    docker_compose: String,
    services: BTreeMap<String, String>,
}

static PROJECTS: Lazy<BTreeMap<String, Project>> = Lazy::new(|| {
    let path = std::env::var("NIRION_PROJECT_FILE")
        .expect("Env var NIRION_PROJECT_FILE must be set");
    let data = fs::read_to_string(path).expect("Failed to read project file");
    serde_json::from_str(&data).expect("Failed to parse project JSON")
});

fn clap_parse_selector(s: &str) -> Result<TargetSelector, String> {
    parse_selector(s, &PROJECTS).map_err(|e| e.to_string())
}

fn clap_parse_image_selector(s: &str) -> Result<ImageSelector, String> {
    parse_image_selector(s, &PROJECTS).map_err(|e| e.to_string())
}

#[derive(Parser)]
#[command(name = "nirion")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Up {
        #[arg(default_value = "*", value_parser = clap_parse_selector)]
        target: TargetSelector,
    },
    Down {
        #[arg(default_value = "*", value_parser = clap_parse_selector)]
        target: TargetSelector,
    },
    List {
        #[arg(help = "If specified, list containers/images in this project")]
        project: Option<String>,
    },
    Update {
        #[arg(default_value = "*", value_parser = clap_parse_selector)]
        target: TargetSelector,
    },
    Lock {
        #[arg(default_value = "*", value_parser = clap_parse_selector)]
        target: TargetSelector,
    },
    Exec {
        #[command(flatten)]
        args: ExecArgs,
    },
    Logs {
        #[command(flatten)]
        args: LogsArgs,
    },
}

fn parse_selector(
    s: &str,
    projects: &BTreeMap<String, Project>,
) -> Result<TargetSelector> {
    let s = s.trim();
    if s == "*" {
        return Ok(TargetSelector::All);
    }

    let parts: Vec<&str> = s.splitn(2, '.').collect();
    match parts.as_slice() {
        [project_name] => {
            if projects.contains_key(*project_name) {
                Ok(TargetSelector::Project(ProjectSelector {
                    name: project_name.to_string(),
                }))
            } else {
                anyhow::bail!("Project '{}' not found", project_name);
            }
        }
        [project_name, image_name] => {
            if let Some(proj) = projects.get(*project_name) {
                if proj.services.contains_key(*image_name) {
                    Ok(TargetSelector::Image(ImageSelector {
                        project: project_name.to_string(),
                        image: image_name.to_string(),
                    }))
                } else {
                    anyhow::bail!(
                        "Image '{}' not found in project '{}'",
                        image_name,
                        project_name
                    );
                }
            } else {
                anyhow::bail!("Project '{}' not found", project_name);
            }
        }
        _ => anyhow::bail!("Invalid target selector: {}", s),
    }
}

fn parse_image_selector(
    s: &str,
    projects: &BTreeMap<String, Project>,
) -> Result<ImageSelector> {
    let selector = parse_selector(s, projects)?;
    match selector {
        TargetSelector::Image(image_selector) => Ok(image_selector),
        _ => anyhow::bail!(
            "Expected image selector like <project>.<image> but got {}",
            s
        ),
    }
}

fn get_env_path(key: &str) -> anyhow::Result<PathBuf> {
    let val = std::env::var(key)
        .map_err(|_| anyhow::anyhow!("Env var {} must be set", key))?;
    val.parse()
        .map_err(|_| anyhow::anyhow!("Failed parsing env var {} as path", key))
}

#[tokio::main]
async fn main() -> Result<()> {
    let lock_file = get_env_path("NIRION_LOCK_FILE")?;
    let locked_images: BTreeMap<String, String> = if lock_file.exists() {
        let lock_file_data = fs::read_to_string(&lock_file)?;
        serde_json::from_str(&lock_file_data)?
    } else {
        BTreeMap::new()
    };

    let cli = Cli::parse();

    match cli.command {
        Commands::List { project } => handle_list(&project, &PROJECTS)?,
        Commands::Up { target } => handle_up(&target, &PROJECTS)?,
        Commands::Down { target } => handle_down(&target, &PROJECTS)?,
        Commands::Update { target } => {
            handle_update(&target, &PROJECTS, &locked_images, &lock_file)
                .await?
        }
        Commands::Lock { target } => {
            handle_lock(&target, &PROJECTS, &locked_images, &lock_file).await?
        }
        Commands::Exec { args } => handle_exec(&args, &PROJECTS)?,
        Commands::Logs { args } => handle_logs(&args, &PROJECTS)?,
    }

    Ok(())
}

fn handle_list(
    project: &Option<String>,
    projects: &BTreeMap<String, Project>,
) -> Result<()> {
    if let Some(project_name) = project {
        if let Some(proj) = projects.get(project_name) {
            println!("Images:");
            for image in proj.services.keys() {
                println!("- {}", image);
            }
        } else {
            println!("Project '{}' not found", project_name);
        }
    } else {
        println!("Projects:");
        for project_name in projects.keys() {
            println!("- {}", project_name);
        }
    }
    Ok(())
}

fn compose_target_cmd(
    target: &TargetSelector,
    projects: &BTreeMap<String, Project>,
    args: &[&str],
) -> Result<()> {
    match target {
        TargetSelector::All => {
            for (project_name, project) in projects {
                let mut cmd_args = vec![
                    "--file",
                    &project.docker_compose,
                    "--project-name",
                    &project_name,
                ];
                cmd_args.extend_from_slice(args);

                println!("Running: docker compose {:?}", cmd_args.join(" "));

                let status = ProcCommand::new("docker-compose")
                    .args(&cmd_args)
                    .status()?;
                if status.success() {
                    println!(
                        "Project '{}' completed successfully.",
                        project_name
                    );
                } else {
                    println!(
                        "Project '{}' failed. Status: {}",
                        project_name, status
                    );
                }
            }
        }
        TargetSelector::Project(proj) => {
            let compose_file = &projects[&proj.name].docker_compose;
            let mut cmd_args =
                vec!["--file", compose_file, "--project-name", &proj.name];
            cmd_args.extend_from_slice(args);

            println!("Running: docker-compose {:?}", cmd_args);

            let status = ProcCommand::new("docker-compose")
                .args(&cmd_args)
                .status()?;
            if status.success() {
                println!("Project '{}' completed successfully.", proj.name);
            } else {
                println!("Project '{}' failed. Status: {}", proj.name, status);
            }
        }
        TargetSelector::Image(img) => {
            let compose_file = &projects[&img.project].docker_compose;
            let mut cmd_args =
                vec!["--file", compose_file, "--project-name", &img.project];
            cmd_args.extend_from_slice(args);
            cmd_args.push(&img.image);

            println!(
                "Running: docker-compose {:?} (image {})",
                cmd_args.join(" "),
                img.image
            );

            let status = ProcCommand::new("docker-compose")
                .args(&cmd_args)
                .status()?;
            if status.success() {
                println!(
                    "Image '{}' in project '{}' completed successfully.",
                    img.image, img.project
                );
            } else {
                println!(
                    "Image '{}' in project '{}' failed. Status: {}",
                    img.image, img.project, status
                );
            }
        }
    }
    Ok(())
}

fn get_images(
    target: &TargetSelector,
    projects: &BTreeMap<String, Project>,
) -> BTreeMap<String, String> {
    let mut images = BTreeMap::new();
    match target {
        TargetSelector::All => {
            for (project_name, project) in projects {
                for (service_name, image) in project.services.iter() {
                    let identifier = format!("{project_name}.{service_name}");
                    images.insert(identifier, image.to_string());
                }
            }
        }
        TargetSelector::Project(proj) => {
            let project = &projects[&proj.name];
            for (service_name, image) in project.services.iter() {
                let identifier = format!("{}.{}", proj.name, service_name);
                images.insert(identifier, image.to_string());
            }
        }
        TargetSelector::Image(img) => {
            let project = &projects[&img.project];
            let image = &project.services[&img.image];
            let identifier = format!("{}.{}", img.project, img.image);
            images.insert(identifier, image.to_string());
        }
    }
    images
}

pub fn fetch_digest(image: &str) -> anyhow::Result<String> {
    let output = ProcCommand::new("skopeo")
        .args([
            "inspect",
            "--format",
            "{{.Digest}}",
            &format!("docker://{}", image),
        ])
        .output()?;

    if !output.status.success() {
        anyhow::bail!("Failed to fetch digest")
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_string())
}
