use crossterm::style::Stylize;
use std::{collections::BTreeMap, process::Stdio};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

use crate::{Project, TargetSelector};

pub async fn compose_target_cmd(
    target: &TargetSelector,
    projects: &BTreeMap<String, Project>,
    args: &[&str],
) -> anyhow::Result<()> {
    match target {
        TargetSelector::All => {
            for (name, project) in projects {
                println!("[{}]", name.to_string().cyan());

                let mut cmd_args = vec![
                    "--file",
                    &project.docker_compose,
                    "--project-name",
                    name,
                ];
                cmd_args.extend_from_slice(args);

                if let Err(e) = run_docker_compose(&cmd_args).await {
                    println!("Project '{}' failed: {}", name, e)
                }

                println!()
            }
        }

        TargetSelector::Project(proj) => {
            let compose_file = &projects[&proj.name].docker_compose;
            let mut cmd_args =
                vec!["--file", compose_file, "--project-name", &proj.name];
            cmd_args.extend_from_slice(args);

            if let Err(e) = run_docker_compose(&cmd_args).await {
                println!("Project '{}' failed: {}", proj.name, e)
            }
        }

        TargetSelector::Image(img) => {
            let compose_file = &projects[&img.project].docker_compose;
            let mut cmd_args =
                vec!["--file", compose_file, "--project-name", &img.project];
            cmd_args.extend_from_slice(args);
            cmd_args.push(&img.image);

            if let Err(e) = run_docker_compose(&cmd_args).await {
                println!("Project '{}' failed: {}", img.project, e)
            }
        }
    }

    Ok(())
}

pub async fn run_docker_compose(cmd_args: &[&str]) -> anyhow::Result<()> {
    let mut child = Command::new("docker-compose")
        .args(cmd_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let out_thread = tokio::spawn(async {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            println!("{}", line);
        }
    });

    let err_thread = tokio::spawn(async {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.contains("the attribute `version` is obsolete") {
                continue;
            }
            println!("{}", line);
        }
    });

    let status = child.wait().await?;

    out_thread.await.ok();
    err_thread.await.ok();

    if !status.success() {
        println!("docker-compose exited with status {}", status);
    }

    Ok(())
}
