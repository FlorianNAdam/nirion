use anyhow::Result;
use std::{collections::BTreeMap, process::Command as ProcCommand};

use crate::{Project, TargetSelector};

pub fn compose_target_cmd(
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
