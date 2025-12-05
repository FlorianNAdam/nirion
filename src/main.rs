use anyhow::Result;
use clap::{Arg, ArgMatches, Command};
use serde::{Deserialize, Serialize};
use std::process::Command as ProcCommand;
use std::{collections::BTreeMap, fs, path::PathBuf};

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
    images: BTreeMap<String, String>,
}

/// Custom parser that resolves projects during arg parsing
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
                if proj.images.contains_key(*image_name) {
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
) -> anyhow::Result<ImageSelector> {
    let selector = parse_selector(s, projects)?;

    match selector {
        TargetSelector::Image(selector) => Ok(selector),
        _ => anyhow::bail!("Expected image selector, but got {s}"),
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
        let lock_file_data = fs::read_to_string(lock_file)?;
        serde_json::from_str(&lock_file_data)?
    } else {
        BTreeMap::new()
    };

    let project_file = get_env_path("NIRION_PROJECT_FILE")?;

    let json_data = fs::read_to_string(&project_file)?;
    let projects: BTreeMap<String, Project> = serde_json::from_str(&json_data)?;

    let matches = Command::new("mycli")
        .about("A small CLI with multiple subcommands")
        .subcommand(
            Command::new("up").about("Bring up a project or image").arg(
                Arg::new("target")
                    .help("Target project or project.image")
                    .default_value("*"),
            ),
        )
        .subcommand(
            Command::new("down")
                .about("Bring down a project or image")
                .arg(
                    Arg::new("target")
                        .help("Target project or project.image")
                        .default_value("*"),
                ),
        )
        .subcommand(
            Command::new("list")
                .about("List projects or containers")
                .arg(
                    Arg::new("project")
                        .help("If specified, list containers/images in this project")
                        .required(false),
                ),
        )
        .subcommand(
            Command::new("update")
                .about("Update a project or image")
                .arg(
                    Arg::new("target")
                        .help("Target project or project.image")
                        .default_value("*"),
                ),
        )
        .get_matches();

    // Resolve subcommands
    match matches.subcommand() {
        Some(("list", sub)) => handle_list(sub, &projects)?,
        Some(("up", sub)) => handle_up(sub, &projects)?,
        Some(("down", sub)) => handle_down(sub, &projects)?,
        Some(("update", sub)) => handle_update(sub, &projects).await?,
        _ => println!("No valid subcommand"),
    }

    Ok(())
}

fn handle_list(
    matches: &ArgMatches,
    projects: &BTreeMap<String, Project>,
) -> Result<()> {
    if let Some(project_name) = matches.get_one::<String>("project") {
        if let Some(proj) = projects.get(project_name) {
            println!("Images:");
            for image in proj.images.keys() {
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

                println!("Running: docker-compose {:?}", cmd_args);

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
                cmd_args, img.image
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

fn handle_up(
    matches: &ArgMatches,
    projects: &BTreeMap<String, Project>,
) -> Result<()> {
    let target_str = matches
        .get_one::<String>("target")
        .expect("required");
    let target = parse_selector(target_str, projects)?;
    compose_target_cmd(&target, projects, &["up", "-d"])
}

fn handle_down(
    matches: &ArgMatches,
    projects: &BTreeMap<String, Project>,
) -> Result<()> {
    let target_str = matches
        .get_one::<String>("target")
        .expect("required");
    let target = parse_selector(target_str, projects)?;
    compose_target_cmd(&target, projects, &["down"])
}

fn get_images(
    target: &TargetSelector,
    projects: &BTreeMap<String, Project>,
) -> BTreeMap<String, String> {
    let mut images = BTreeMap::new();
    match target {
        TargetSelector::All => {
            for (project_name, project) in projects {
                for (service_name, image) in project.images.iter() {
                    let identifier = format!("{project_name}.{service_name}");
                    images.insert(identifier, image.to_string());
                }
            }
        }
        TargetSelector::Project(proj) => {
            let project = &projects[&proj.name];
            for (service_name, image) in project.images.iter() {
                let project_name = &proj.name;
                let identifier = format!("{project_name}.{service_name}");
                images.insert(identifier, image.to_string());
            }
        }
        TargetSelector::Image(img) => {
            let project_name = &img.project;
            let service_name = &img.image;
            let project = &projects[project_name];
            let image = &project.images[service_name];
            let identifier = format!("{project_name}.{service_name}");
            images.insert(identifier, image.to_string());
        }
    }
    images
}

async fn handle_update(
    matches: &ArgMatches,
    projects: &BTreeMap<String, Project>,
) -> Result<()> {
    let target_str = matches
        .get_one::<String>("target")
        .expect("required");
    let target = parse_selector(target_str, projects)?;

    let images = get_images(&target, projects);

    for (service, image) in images {
        let digest = fetch_digest(&image)?;
        println!("{}", digest);
    }

    Ok(())
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
