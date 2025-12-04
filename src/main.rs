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
fn parse_selector(s: &str, projects: &BTreeMap<String, Project>) -> Result<TargetSelector> {
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
    let val = std::env::var(key).map_err(|_| anyhow::anyhow!("Env var {} must be set", key))?;
    val.parse()
        .map_err(|_| anyhow::anyhow!("Failed parsing env var {} as path", key))
}

fn main() -> Result<()> {
    let lock_file = get_env_path("NIRION_LOCK_FILE")?;

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
        .get_matches();

    // Resolve subcommands
    match matches.subcommand() {
        Some(("list", sub)) => handle_list(sub, &projects)?,
        Some(("up", sub)) => handle_up(sub, &projects)?,
        Some(("down", sub)) => handle_down(sub, &projects)?,
        _ => println!("No valid subcommand"),
    }

    Ok(())
}

fn handle_list(matches: &ArgMatches, projects: &BTreeMap<String, Project>) -> Result<()> {
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

fn run_arion_target_cmd(
    target: &TargetSelector,
    projects: &BTreeMap<String, Project>,
    args: &[&str],
) -> Result<()> {
    match target {
        TargetSelector::All => {
            for (project_name, project) in projects {
                let mut cmd_args = vec!["--prebuilt-file", &project.docker_compose];
                cmd_args.extend_from_slice(args);

                println!("Running: arion {:?}", cmd_args);

                let status = ProcCommand::new("arion").args(&cmd_args).status()?;
                if status.success() {
                    println!("Project '{}' completed successfully.", project_name);
                } else {
                    println!("Project '{}' failed. Status: {}", project_name, status);
                }
            }
        }
        TargetSelector::Project(proj) => {
            let compose_file = &projects[&proj.name].docker_compose;
            let mut cmd_args = vec!["--prebuilt-file", compose_file];
            cmd_args.extend_from_slice(args);

            println!("Running: arion {:?}", cmd_args);

            let status = ProcCommand::new("arion").args(&cmd_args).status()?;
            if status.success() {
                println!("Project '{}' completed successfully.", proj.name);
            } else {
                println!("Project '{}' failed. Status: {}", proj.name, status);
            }
        }
        TargetSelector::Image(img) => {
            let compose_file = &projects[&img.project].docker_compose;
            let mut cmd_args = vec!["--prebuilt-file", compose_file];
            cmd_args.extend_from_slice(args);

            println!("Running: arion {:?} (image {})", cmd_args, img.image);

            let status = ProcCommand::new("arion").args(&cmd_args).status()?;
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

fn handle_up(matches: &ArgMatches, projects: &BTreeMap<String, Project>) -> Result<()> {
    let target_str = matches.get_one::<String>("target").expect("required");
    let target = parse_selector(target_str, projects)?;
    run_arion_target_cmd(&target, projects, &["up"])
}

fn handle_down(matches: &ArgMatches, projects: &BTreeMap<String, Project>) -> Result<()> {
    let target_str = matches.get_one::<String>("target").expect("required");
    let target = parse_selector(target_str, projects)?;
    run_arion_target_cmd(&target, projects, &["down"])
}
