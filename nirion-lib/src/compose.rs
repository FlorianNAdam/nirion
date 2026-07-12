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

#[cfg(test)]
static TEST_DOCKER_BIN: std::sync::Mutex<Option<String>> =
    std::sync::Mutex::new(None);

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
    run_docker_compose(build_compose_args(compose_file, project_name, args))
}

fn build_compose_args(
    compose_file: String,
    project_name: ProjectName,
    args: Vec<String>,
) -> Vec<String> {
    let mut cmd_args = vec![
        "--file".to_string(),
        compose_file,
        "--project-name".to_string(),
        project_name.deref().to_string(),
    ];
    cmd_args.extend(args);

    cmd_args
}

pub fn run_docker_compose(
    cmd_args: Vec<String>,
) -> BoxStream<'static, anyhow::Result<ProcessEvent>> {
    let (tx, rx) = mpsc::unbounded();

    tokio::spawn(async move {
        let mut child = match docker_command()
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

fn docker_command() -> Command {
    #[cfg(test)]
    if let Some(bin) = TEST_DOCKER_BIN.lock().unwrap().clone() {
        return Command::new(bin);
    }

    Command::new("docker")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt, path::Path};

    static DOCKER_BIN_LOCK: tokio::sync::Mutex<()> =
        tokio::sync::Mutex::const_new(());

    struct DockerBinGuard;

    impl DockerBinGuard {
        fn set(bin: String) -> Self {
            *TEST_DOCKER_BIN.lock().unwrap() = Some(bin);
            Self
        }
    }

    impl Drop for DockerBinGuard {
        fn drop(&mut self) {
            *TEST_DOCKER_BIN.lock().unwrap() = None;
        }
    }

    async fn collect_events(
        stream: BoxStream<'static, anyhow::Result<ProcessEvent>>,
    ) -> Vec<anyhow::Result<ProcessEvent>> {
        stream.collect::<Vec<_>>().await
    }

    fn write_fake_docker(
        dir: &Path,
        args_file: &Path,
        exit_code: i32,
    ) -> String {
        let docker = dir.join("docker");
        fs::write(
            &docker,
            format!(
                r#"#!/bin/sh
printf '%s\n' "$@" > '{}'
echo stdout-line
echo stderr-line >&2
exit {exit_code}
"#,
                args_file.display()
            ),
        )
        .unwrap();

        let mut permissions = fs::metadata(&docker)
            .unwrap()
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&docker, permissions).unwrap();

        docker.to_string_lossy().to_string()
    }

    #[test]
    fn build_compose_args_adds_file_and_project_name() {
        let args = build_compose_args(
            "compose.yml".into(),
            ProjectName("myapp".into()),
            vec!["up".into(), "-d".into()],
        );

        assert_eq!(
            args,
            vec![
                "--file",
                "compose.yml",
                "--project-name",
                "myapp",
                "up",
                "-d"
            ]
        );
    }

    #[test]
    fn build_compose_args_keeps_empty_passthrough_args_empty() {
        let args = build_compose_args(
            "compose.json".into(),
            ProjectName("api".into()),
            Vec::new(),
        );

        assert_eq!(
            args,
            vec!["--file", "compose.json", "--project-name", "api"]
        );
    }

    #[tokio::test]
    async fn run_docker_compose_streams_output_and_exit_status() {
        let _docker_bin_lock = DOCKER_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 0);
        let _docker_bin_guard = DockerBinGuard::set(docker);

        let events = collect_events(run_docker_compose(vec![
            "ps".into(),
            "--format".into(),
            "json".into(),
        ]))
        .await;

        assert!(events.iter().any(|event| matches!(
            event,
            Ok(ProcessEvent::StdoutLine(line)) if line == "stdout-line"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            Ok(ProcessEvent::StderrLine(line)) if line == "stderr-line"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            Ok(ProcessEvent::Exited(status)) if status.code == Some(0) && status.success
        )));
        assert!(events.iter().all(Result::is_ok));

        assert_eq!(
            fs::read_to_string(args_file).unwrap(),
            "compose\nps\n--format\njson\n"
        );
    }

    #[tokio::test]
    async fn run_docker_compose_emits_error_for_failed_exit_status() {
        let _docker_bin_lock = DOCKER_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 42);
        let _docker_bin_guard = DockerBinGuard::set(docker);

        let events =
            collect_events(run_docker_compose(vec!["up".into()])).await;

        assert!(events.iter().any(|event| matches!(
            event,
            Ok(ProcessEvent::Exited(status)) if status.code == Some(42) && !status.success
        )));
        assert!(events.iter().any(|event| {
            match event {
                Err(err) => err
                    .to_string()
                    .contains("docker compose exited with status"),
                Ok(_) => false,
            }
        }));
    }
}
