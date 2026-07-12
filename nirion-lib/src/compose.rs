use std::{ops::Deref, process::Stdio};

use anyhow::Context;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

use crate::projects::ProjectName;

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
    let mut child = Command::new("docker")
        .arg("compose")
        .args(cmd_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to execute docker compose")?;

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
        anyhow::bail!("docker compose exited with status {}", status);
    }

    Ok(())
}
