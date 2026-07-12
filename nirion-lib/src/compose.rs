use std::{ops::Deref, process::Stdio};

use anyhow::Context;
use futures::{StreamExt, channel::mpsc, stream::BoxStream};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

use crate::{
    events::{ComposeEvent, ProcessEvent},
    projects::{ProjectName, Projects, TargetSelector},
};

pub fn compose_target(
    target: TargetSelector,
    projects: Projects,
    args: Vec<String>,
) -> BoxStream<'static, anyhow::Result<ComposeEvent>> {
    let (tx, rx) = mpsc::unbounded();

    tokio::spawn(async move {
        let mut failures = Vec::new();

        match target {
            TargetSelector::All => {
                for (name, project) in projects.iter() {
                    let _ =
                        tx.unbounded_send(Ok(ComposeEvent::ProjectStarted {
                            project: name.to_string(),
                        }));

                    let mut stream = compose_cmd(
                        project.docker_compose.clone(),
                        project.name.clone(),
                        args.clone(),
                    );

                    while let Some(event) = stream.next().await {
                        match event {
                            Ok(event) => {
                                let _ = tx.unbounded_send(Ok(
                                    ComposeEvent::Process {
                                        project: Some(name.to_string()),
                                        event,
                                    },
                                ));
                            }
                            Err(e) => {
                                let error = e.to_string();
                                let _ = tx.unbounded_send(Ok(
                                    ComposeEvent::ProjectFailed {
                                        project: name.to_string(),
                                        error: error.clone(),
                                    },
                                ));
                                failures.push(format!("{name}: {error}"));
                            }
                        }
                    }
                }
            }
            TargetSelector::Project(proj) => {
                let project = projects[&proj.name].clone();
                let mut stream = compose_cmd(
                    project.docker_compose,
                    project.name,
                    args.clone(),
                );

                while let Some(event) = stream.next().await {
                    match event {
                        Ok(event) => {
                            let _ =
                                tx.unbounded_send(Ok(ComposeEvent::Process {
                                    project: Some(proj.name.clone()),
                                    event,
                                }));
                        }
                        Err(e) => {
                            let _ = tx.unbounded_send(Err(e.context(format!(
                                "Project '{}' failed",
                                proj.name
                            ))));
                            return;
                        }
                    }
                }
            }
            TargetSelector::Service(sel) => {
                let project = projects[&sel.project].clone();
                let mut cmd_args = args.clone();
                cmd_args.push(sel.service.clone());

                let mut stream =
                    compose_cmd(project.docker_compose, project.name, cmd_args);

                while let Some(event) = stream.next().await {
                    match event {
                        Ok(event) => {
                            let _ =
                                tx.unbounded_send(Ok(ComposeEvent::Process {
                                    project: Some(sel.project.clone()),
                                    event,
                                }));
                        }
                        Err(e) => {
                            let _ = tx.unbounded_send(Err(anyhow::anyhow!(
                                "Service '{}.{}' failed: {}",
                                sel.project,
                                sel.service,
                                e
                            )));
                            return;
                        }
                    }
                }
            }
        }

        if !failures.is_empty() {
            let _ = tx.unbounded_send(Err(anyhow::anyhow!(
                "docker compose failed for {} project(s): {}",
                failures.len(),
                failures.join("; ")
            )));
        }
    });

    rx.boxed()
}

pub fn compose_cmd(
    compose_file: String,
    project_name: ProjectName,
    args: Vec<String>,
) -> BoxStream<'static, anyhow::Result<ProcessEvent>> {
    let mut cmd_args = vec![
        "--file".to_string(),
        compose_file,
        "--project-name".to_string(),
        project_name.deref().to_string(),
    ];
    cmd_args.extend(args);

    run_docker_compose(cmd_args)
}

pub fn run_docker_compose(
    cmd_args: Vec<String>,
) -> BoxStream<'static, anyhow::Result<ProcessEvent>> {
    let (tx, rx) = mpsc::unbounded();

    tokio::spawn(async move {
        let mut child = match Command::new("docker")
            .arg("compose")
            .args(cmd_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to execute docker compose")
        {
            Ok(child) => child,
            Err(e) => {
                let _ = tx.unbounded_send(Err(e));
                return;
            }
        };

        let Some(stdout) = child.stdout.take() else {
            let _ = tx.unbounded_send(Err(anyhow::anyhow!(
                "failed to capture stdout"
            )));
            return;
        };
        let Some(stderr) = child.stderr.take() else {
            let _ = tx.unbounded_send(Err(anyhow::anyhow!(
                "failed to capture stderr"
            )));
            return;
        };

        let out_tx = tx.clone();
        let out_thread = tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ =
                    out_tx.unbounded_send(Ok(ProcessEvent::StdoutLine(line)));
            }
        });

        let err_tx = tx.clone();
        let err_thread = tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ =
                    err_tx.unbounded_send(Ok(ProcessEvent::StderrLine(line)));
            }
        });

        let status = match child.wait().await {
            Ok(status) => status,
            Err(e) => {
                let _ = tx.unbounded_send(Err(e.into()));
                return;
            }
        };

        out_thread.await.ok();
        err_thread.await.ok();

        let _ = tx.unbounded_send(Ok(ProcessEvent::Exited(status.into())));

        if !status.success() {
            let _ = tx.unbounded_send(Err(anyhow::anyhow!(
                "docker compose exited with status {}",
                status
            )));
        }
    });

    rx.boxed()
}
