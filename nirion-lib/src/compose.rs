use std::{
    ops::Deref,
    path::{Path, PathBuf},
    process::Stdio,
};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposeConcurrency {
    Sequential,
    Parallel,
}

#[cfg(test)]
static TEST_DOCKER_CMD: std::sync::Mutex<Option<Vec<String>>> =
    std::sync::Mutex::new(None);

pub fn compose_target(
    target: TargetSelector,
    projects: Projects,
    args: Vec<String>,
) -> BoxStream<'static, anyhow::Result<ComposeEvent>> {
    compose_target_with_docker(PathBuf::from("docker"), target, projects, args)
}

pub fn compose_target_with_docker(
    docker_binary: PathBuf,
    target: TargetSelector,
    projects: Projects,
    args: Vec<String>,
) -> BoxStream<'static, anyhow::Result<ComposeEvent>> {
    compose_target_with_concurrency(
        docker_binary,
        target,
        projects,
        args,
        ComposeConcurrency::Sequential,
    )
}

pub fn compose_target_with_concurrency(
    docker_binary: PathBuf,
    target: TargetSelector,
    projects: Projects,
    args: Vec<String>,
    concurrency: ComposeConcurrency,
) -> BoxStream<'static, anyhow::Result<ComposeEvent>> {
    match concurrency {
        ComposeConcurrency::Sequential => {
            compose_target_sequential(docker_binary, target, projects, args)
        }
        ComposeConcurrency::Parallel => match target {
            TargetSelector::All => {
                compose_target_all_parallel(docker_binary, projects, args)
            }
            target => {
                compose_target_sequential(docker_binary, target, projects, args)
            }
        },
    }
}

fn compose_target_sequential(
    docker_binary: PathBuf,
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
                        docker_binary.clone(),
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
                    docker_binary.clone(),
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

                let mut stream = compose_cmd(
                    docker_binary,
                    project.docker_compose,
                    project.name,
                    cmd_args,
                );

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

fn compose_target_all_parallel(
    docker_binary: PathBuf,
    projects: Projects,
    args: Vec<String>,
) -> BoxStream<'static, anyhow::Result<ComposeEvent>> {
    let (tx, rx) = mpsc::unbounded();

    tokio::spawn(async move {
        let mut handles = Vec::new();

        for (name, project) in projects.iter() {
            let name = name.to_string();
            let project = project.clone();
            let args = args.clone();
            let docker_binary = docker_binary.clone();
            let tx = tx.clone();

            let _ = tx.unbounded_send(Ok(ComposeEvent::ProjectStarted {
                project: name.clone(),
            }));

            handles.push(tokio::spawn(async move {
                let mut stream = compose_cmd(
                    docker_binary,
                    project.docker_compose,
                    project.name,
                    args,
                );

                while let Some(event) = stream.next().await {
                    match event {
                        Ok(event) => {
                            let _ =
                                tx.unbounded_send(Ok(ComposeEvent::Process {
                                    project: Some(name.clone()),
                                    event,
                                }));
                        }
                        Err(e) => {
                            let error = e.to_string();
                            let _ = tx.unbounded_send(Ok(
                                ComposeEvent::ProjectFailed {
                                    project: name.clone(),
                                    error: error.clone(),
                                },
                            ));
                            return Some(format!("{name}: {error}"));
                        }
                    }
                }

                None
            }));
        }

        let mut failures = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(Some(failure)) => failures.push(failure),
                Ok(None) => {}
                Err(error) => failures.push(error.to_string()),
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
    docker_binary: PathBuf,
    compose_file: String,
    project_name: ProjectName,
    args: Vec<String>,
) -> BoxStream<'static, anyhow::Result<ProcessEvent>> {
    run_docker_compose(
        docker_binary,
        build_compose_args(compose_file, project_name, args),
    )
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
    docker_binary: PathBuf,
    cmd_args: Vec<String>,
) -> BoxStream<'static, anyhow::Result<ProcessEvent>> {
    let (tx, rx) = mpsc::unbounded();

    tokio::spawn(async move {
        let mut child = match docker_command(&docker_binary)
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
            let mut stderr = Vec::new();
            while let Ok(Some(line)) = lines.next_line().await {
                stderr.push(line.clone());
                let _ =
                    err_tx.unbounded_send(Ok(ProcessEvent::StderrLine(line)));
            }

            stderr
        });

        let status = match child.wait().await {
            Ok(status) => status,
            Err(e) => {
                let _ = tx.unbounded_send(Err(e.into()));
                return;
            }
        };

        out_thread.await.ok();
        let stderr = err_thread.await.unwrap_or_default();

        let _ = tx.unbounded_send(Ok(ProcessEvent::Exited(status.into())));

        if !status.success() {
            let stderr = stderr.join("\n");
            let stderr = stderr.trim();
            let _ = tx.unbounded_send(Err(if stderr.is_empty() {
                anyhow::anyhow!("docker compose exited with status {}", status)
            } else {
                anyhow::anyhow!(
                    "docker compose exited with status {}: {}",
                    status,
                    stderr
                )
            }));
        }
    });

    rx.boxed()
}

fn docker_command(docker_binary: &Path) -> Command {
    #[cfg(test)]
    if let Some(cmd) = TEST_DOCKER_CMD.lock().unwrap().clone() {
        let mut command = Command::new(&cmd[0]);
        command.args(&cmd[1..]);
        return command;
    }

    Command::new(docker_binary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt, path::Path};

    static DOCKER_BIN_LOCK: tokio::sync::Mutex<()> =
        tokio::sync::Mutex::const_new(());

    struct DockerBinGuard;

    impl DockerBinGuard {
        fn set_script(script: String) -> Self {
            *TEST_DOCKER_CMD.lock().unwrap() =
                Some(vec!["/bin/sh".to_string(), script]);
            Self
        }

        fn set_command(command: String) -> Self {
            *TEST_DOCKER_CMD.lock().unwrap() = Some(vec![command]);
            Self
        }
    }

    impl Drop for DockerBinGuard {
        fn drop(&mut self) {
            *TEST_DOCKER_CMD.lock().unwrap() = None;
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

    fn projects() -> Projects {
        serde_json::from_value(serde_json::json!({
            "api": {
                "name": "api",
                "dockerCompose": "api.yml",
                "services": {
                    "web": {
                        "image": "nginx",
                        "healthcheck": false,
                        "restart": null
                    }
                }
            },
            "worker": {
                "name": "worker",
                "dockerCompose": "worker.yml",
                "services": {
                    "jobs": {
                        "image": "busybox",
                        "healthcheck": false,
                        "restart": null
                    }
                }
            }
        }))
        .unwrap()
    }

    async fn collect_compose_events(
        stream: BoxStream<'static, anyhow::Result<ComposeEvent>>,
    ) -> Vec<anyhow::Result<ComposeEvent>> {
        stream.collect::<Vec<_>>().await
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
        let _docker_bin_guard = DockerBinGuard::set_script(docker);

        let events = collect_events(run_docker_compose(
            PathBuf::from("docker"),
            vec!["ps".into(), "--format".into(), "json".into()],
        ))
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
        let _docker_bin_guard = DockerBinGuard::set_script(docker);

        let events = collect_events(run_docker_compose(
            PathBuf::from("docker"),
            vec!["up".into()],
        ))
        .await;

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

    #[tokio::test]
    async fn run_docker_compose_reports_spawn_failure() {
        let _docker_bin_lock = DOCKER_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let missing_docker = dir.path().join("missing-docker");
        let _docker_bin_guard = DockerBinGuard::set_command(
            missing_docker
                .to_string_lossy()
                .to_string(),
        );

        let events = collect_events(run_docker_compose(
            PathBuf::from("docker"),
            vec!["ps".into()],
        ))
        .await;

        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], Err(err) if err.to_string().contains("failed to execute docker compose"))
        );
    }

    #[tokio::test]
    async fn compose_cmd_builds_args_and_streams_events() {
        let _docker_bin_lock = DOCKER_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 0);
        let _docker_bin_guard = DockerBinGuard::set_script(docker);

        let events = collect_events(compose_cmd(
            PathBuf::from("docker"),
            "compose.yml".into(),
            ProjectName("myapp".into()),
            vec!["logs".into()],
        ))
        .await;

        assert!(events.iter().all(Result::is_ok));
        assert_eq!(
            fs::read_to_string(args_file).unwrap(),
            "compose\n--file\ncompose.yml\n--project-name\nmyapp\nlogs\n"
        );
    }

    #[tokio::test]
    async fn compose_target_project_wraps_process_events() {
        let _docker_bin_lock = DOCKER_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 0);
        let _docker_bin_guard = DockerBinGuard::set_script(docker);

        let events = collect_compose_events(compose_target(
            TargetSelector::Project(crate::projects::ProjectSelector {
                name: "api".into(),
            }),
            projects(),
            vec!["up".into(), "-d".into()],
        ))
        .await;

        assert!(events.iter().any(|event| matches!(
            event,
            Ok(ComposeEvent::Process {
                project: Some(project),
                event: ProcessEvent::StdoutLine(line),
            }) if project == "api" && line == "stdout-line"
        )));
        assert!(events.iter().all(Result::is_ok));
        assert_eq!(
            fs::read_to_string(args_file).unwrap(),
            "compose\n--file\napi.yml\n--project-name\napi\nup\n-d\n"
        );
    }

    #[tokio::test]
    async fn compose_target_service_appends_service_name() {
        let _docker_bin_lock = DOCKER_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 0);
        let _docker_bin_guard = DockerBinGuard::set_script(docker);

        let events = collect_compose_events(compose_target(
            TargetSelector::Service(crate::projects::ServiceSelector {
                project: "api".into(),
                service: "web".into(),
            }),
            projects(),
            vec!["restart".into()],
        ))
        .await;

        assert!(events.iter().all(Result::is_ok));
        assert_eq!(
            fs::read_to_string(args_file).unwrap(),
            "compose\n--file\napi.yml\n--project-name\napi\nrestart\nweb\n"
        );
    }

    #[tokio::test]
    async fn compose_target_service_reports_failure() {
        let _docker_bin_lock = DOCKER_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 9);
        let _docker_bin_guard = DockerBinGuard::set_script(docker);

        let events = collect_compose_events(compose_target(
            TargetSelector::Service(crate::projects::ServiceSelector {
                project: "api".into(),
                service: "web".into(),
            }),
            projects(),
            vec!["restart".into()],
        ))
        .await;

        assert!(events.iter().any(|event| {
            match event {
                Err(err) => err
                    .to_string()
                    .contains("Service 'api.web' failed"),
                Ok(_) => false,
            }
        }));
    }

    #[tokio::test]
    async fn compose_target_all_emits_project_boundaries() {
        let _docker_bin_lock = DOCKER_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 0);
        let _docker_bin_guard = DockerBinGuard::set_script(docker);

        let events = collect_compose_events(compose_target(
            TargetSelector::All,
            projects(),
            vec!["pull".into()],
        ))
        .await;

        assert!(events.iter().any(|event| matches!(
            event,
            Ok(ComposeEvent::ProjectStarted { project }) if project == "api"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            Ok(ComposeEvent::ProjectStarted { project }) if project == "worker"
        )));
        assert!(events.iter().all(Result::is_ok));
    }

    #[tokio::test]
    async fn compose_target_parallel_all_emits_project_boundaries() {
        let _docker_bin_lock = DOCKER_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 0);
        let _docker_bin_guard = DockerBinGuard::set_script(docker);

        let events = collect_compose_events(compose_target_with_concurrency(
            PathBuf::from("docker"),
            TargetSelector::All,
            projects(),
            vec!["pull".into()],
            ComposeConcurrency::Parallel,
        ))
        .await;

        assert!(events.iter().any(|event| matches!(
            event,
            Ok(ComposeEvent::ProjectStarted { project }) if project == "api"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            Ok(ComposeEvent::ProjectStarted { project }) if project == "worker"
        )));
        assert!(events.iter().all(Result::is_ok));
    }

    #[tokio::test]
    async fn compose_target_project_reports_failure() {
        let _docker_bin_lock = DOCKER_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 2);
        let _docker_bin_guard = DockerBinGuard::set_script(docker);

        let events = collect_compose_events(compose_target(
            TargetSelector::Project(crate::projects::ProjectSelector {
                name: "api".into(),
            }),
            projects(),
            vec!["up".into()],
        ))
        .await;

        assert!(events.iter().any(|event| {
            match event {
                Err(err) => err
                    .to_string()
                    .contains("Project 'api' failed"),
                Ok(_) => false,
            }
        }));
    }

    #[tokio::test]
    async fn compose_target_all_collects_failures_and_continues() {
        let _docker_bin_lock = DOCKER_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 5);
        let _docker_bin_guard = DockerBinGuard::set_script(docker);

        let events = collect_compose_events(compose_target(
            TargetSelector::All,
            projects(),
            vec!["up".into()],
        ))
        .await;

        let project_failures = events
            .iter()
            .filter(|event| {
                matches!(event, Ok(ComposeEvent::ProjectFailed { .. }))
            })
            .count();
        assert_eq!(project_failures, 2);
        assert!(events.iter().any(|event| {
            match event {
                Err(err) => err
                    .to_string()
                    .contains("docker compose failed for 2 project(s)"),
                Ok(_) => false,
            }
        }));
    }

    #[tokio::test]
    async fn compose_target_parallel_all_collects_failures() {
        let _docker_bin_lock = DOCKER_BIN_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 5);
        let _docker_bin_guard = DockerBinGuard::set_script(docker);

        let events = collect_compose_events(compose_target_with_concurrency(
            PathBuf::from("docker"),
            TargetSelector::All,
            projects(),
            vec!["up".into()],
            ComposeConcurrency::Parallel,
        ))
        .await;

        let project_failures = events
            .iter()
            .filter(|event| {
                matches!(event, Ok(ComposeEvent::ProjectFailed { .. }))
            })
            .count();
        assert_eq!(project_failures, 2);
        assert!(events.iter().any(|event| {
            match event {
                Err(err) => err
                    .to_string()
                    .contains("docker compose failed for 2 project(s)"),
                Ok(_) => false,
            }
        }));
    }
}
