use crossterm::style::Stylize;
use std::{collections::BTreeMap, ops::Deref, process::Stdio};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

use crate::{Project, ProjectName, TargetSelector};

pub async fn compose_target_cmd(
    target: &TargetSelector,
    projects: &BTreeMap<String, Project>,
    args: &[&str],
) -> anyhow::Result<()> {
    match target {
        TargetSelector::All => {
            for (name, project) in projects {
                println!("[{}]", name.to_string().cyan());

                let compose_file = &project.docker_compose;
                let project_name = &project.name;

                if let Err(e) =
                    compose_cmd(compose_file, project_name, &args).await
                {
                    println!("Project '{}' failed: {}", name, e)
                }

                println!()
            }
        }

        TargetSelector::Project(proj) => {
            let project = &projects[&proj.name];

            let compose_file = &project.docker_compose;
            let project_name = &project.name;

            if let Err(e) = compose_cmd(compose_file, project_name, &args).await
            {
                println!("Project '{}' failed: {}", proj.name, e)
            }
        }

        TargetSelector::Service(img) => {
            let project = &projects[&img.project];

            let compose_file = &project.docker_compose;
            let project_name = &project.name;

            let mut cmd_args = args.to_vec();
            cmd_args.push(&img.service);

            if let Err(e) =
                compose_cmd(compose_file, project_name, &cmd_args).await
            {
                println!(
                    "Service '{}.{}' failed: {}",
                    img.project, img.service, e
                )
            }
        }
    }

    Ok(())
}

pub async fn compose_cmd(
    compose_file: &str,
    project_name: &ProjectName,
    args: &[&str],
) -> anyhow::Result<()> {
    let mut cmd_args = vec![
        "--file",
        compose_file,
        "--project-name",
        project_name.deref(),
    ];
    cmd_args.extend_from_slice(args);

    run_docker_compose(&cmd_args).await
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
