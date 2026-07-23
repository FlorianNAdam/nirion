use std::{ops::Deref, process::Stdio};

use anyhow::Context;
use futures::{StreamExt, channel::mpsc, stream, stream::BoxStream};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::{
    context::NirionContext,
    events::{ComposeEvent, ProcessEvent},
    projects::{ProjectName, TargetSelector},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposeConcurrency {
    Jobs(usize),
}

impl ComposeConcurrency {
    pub fn sequential() -> Self {
        Self::Jobs(1)
    }

    pub fn unbounded() -> Self {
        Self::Jobs(usize::MAX)
    }

    fn jobs(self) -> usize {
        match self {
            Self::Jobs(jobs) => jobs.max(1),
        }
    }
}

pub fn compose_stream(
    context: NirionContext,
    target: TargetSelector,
    args: Vec<String>,
    concurrency: ComposeConcurrency,
) -> BoxStream<'static, anyhow::Result<ComposeEvent>> {
    let jobs = concurrency.jobs();

    match target {
        TargetSelector::All => compose_stream_all(context, args, jobs),
        target => compose_stream_single(context, target, args),
    }
}

fn compose_stream_single(
    context: NirionContext,
    target: TargetSelector,
    args: Vec<String>,
) -> BoxStream<'static, anyhow::Result<ComposeEvent>> {
    let (tx, rx) = mpsc::unbounded();

    tokio::spawn(async move {
        match target {
            TargetSelector::All => {
                unreachable!("all targets use compose_stream_all")
            }
            TargetSelector::Project(proj) => {
                let project = context.projects[&proj.name].clone();
                let mut stream = compose_cmd(
                    context,
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
                let project = context.projects[&sel.project].clone();
                let mut cmd_args = args.clone();
                cmd_args.push(sel.service.clone());

                let mut stream = compose_cmd(
                    context,
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
    });

    rx.boxed()
}

fn compose_stream_all(
    context: NirionContext,
    args: Vec<String>,
    jobs: usize,
) -> BoxStream<'static, anyhow::Result<ComposeEvent>> {
    let (tx, rx) = mpsc::unbounded();

    tokio::spawn(async move {
        let projects = context
            .projects
            .iter()
            .map(|(name, project)| (name.to_string(), project.clone()))
            .collect::<Vec<_>>();

        let failures = stream::iter(projects)
            .map(|(name, project)| {
                let args = args.clone();
                let context = context.clone();
                let tx = tx.clone();

                async move {
                    let _ =
                        tx.unbounded_send(Ok(ComposeEvent::ProjectStarted {
                            project: name.clone(),
                        }));

                    let mut stream = compose_cmd(
                        context,
                        project.docker_compose,
                        project.name,
                        args,
                    );

                    while let Some(event) = stream.next().await {
                        match event {
                            Ok(event) => {
                                let _ = tx.unbounded_send(Ok(
                                    ComposeEvent::Process {
                                        project: Some(name.clone()),
                                        event,
                                    },
                                ));
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
                }
            })
            .buffer_unordered(jobs)
            .filter_map(|failure| async move { failure })
            .collect::<Vec<_>>()
            .await;

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

fn compose_cmd(
    context: NirionContext,
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

    run_docker_compose(context, cmd_args)
}

fn run_docker_compose(
    context: NirionContext,
    cmd_args: Vec<String>,
) -> BoxStream<'static, anyhow::Result<ProcessEvent>> {
    let (tx, rx) = mpsc::unbounded();

    tokio::spawn(async move {
        let mut child = match context
            .docker_command
            .command()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::Projects;
    use crate::{docker::DockerCommand, lock::LockedImages};
    use nirion_oci_lib::client::NirionOciClient;
    use std::{
        fs, os::unix::fs::PermissionsExt, path::Path, path::PathBuf, sync::Arc,
    };

    async fn collect_events(
        stream: BoxStream<'static, anyhow::Result<ProcessEvent>>
    ) -> Vec<anyhow::Result<ProcessEvent>> {
        stream.collect::<Vec<_>>().await
    }

    fn write_fake_docker(
        dir: &Path,
        args_file: &Path,
        exit_code: i32,
    ) -> String {
        let docker = dir.join("docker");
        let tmp = dir.join("docker.tmp");
        let mut file = fs::File::create(&tmp).unwrap();
        use std::io::Write;
        file.write_all(
            format!(
                r#"#!/bin/sh
printf '%s\n' "$@" > '{}'
echo stdout-line
echo stderr-line >&2
exit {exit_code}
"#,
                args_file.display()
            )
            .as_bytes(),
        )
        .unwrap();
        file.sync_all().unwrap();
        drop(file);

        let mut permissions = fs::metadata(&tmp)
            .unwrap()
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&tmp, permissions).unwrap();
        fs::rename(&tmp, &docker).unwrap();

        docker.to_string_lossy().to_string()
    }

    fn fake_docker_command(script: &str) -> DockerCommand {
        DockerCommand::with_args("/bin/sh", [script])
    }

    fn context(docker_command: DockerCommand) -> NirionContext {
        NirionContext {
            projects: projects(),
            locked_images: LockedImages::default(),
            lock_file: PathBuf::from("lock.json"),
            oci_client: Arc::new(NirionOciClient::builder().build()),
            docker_command,
        }
    }

    fn write_timed_fake_docker(
        dir: &Path,
        log_file: &Path,
    ) -> String {
        let docker = dir.join("docker-timed");
        let tmp = dir.join("docker-timed.tmp");
        let mut file = fs::File::create(&tmp).unwrap();
        use std::io::Write;
        file.write_all(
            format!(
                r#"#!/bin/sh
project=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--project-name" ]; then
    shift
    project="$1"
  fi
  shift
done
printf 'start %s\n' "$project" >> '{}'
sleep 0.2
printf 'end %s\n' "$project" >> '{}'
exit 0
"#,
                log_file.display(),
                log_file.display(),
            )
            .as_bytes(),
        )
        .unwrap();
        file.sync_all().unwrap();
        drop(file);

        let mut permissions = fs::metadata(&tmp)
            .unwrap()
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&tmp, permissions).unwrap();
        fs::rename(&tmp, &docker).unwrap();

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
                        "resolvedImage": "nginx@sha256:abc",
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
                        "resolvedImage": "busybox@sha256:def",
                        "healthcheck": false,
                        "restart": null
                    }
                }
            }
        }))
        .unwrap()
    }

    async fn collect_compose_events(
        stream: BoxStream<'static, anyhow::Result<ComposeEvent>>
    ) -> Vec<anyhow::Result<ComposeEvent>> {
        stream.collect::<Vec<_>>().await
    }

    #[tokio::test]
    async fn run_docker_compose_streams_output_and_exit_status() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 0);

        let events = collect_events(run_docker_compose(
            context(fake_docker_command(&docker)),
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
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 42);

        let events = collect_events(run_docker_compose(
            context(fake_docker_command(&docker)),
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
        let dir = tempfile::tempdir().unwrap();
        let missing_docker = dir.path().join("missing-docker");

        let events = collect_events(run_docker_compose(
            context(DockerCommand::new(missing_docker)),
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
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 0);

        let events = collect_events(compose_cmd(
            context(fake_docker_command(&docker)),
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
    async fn compose_stream_project_wraps_process_events() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 0);

        let events = collect_compose_events(compose_stream(
            context(fake_docker_command(&docker)),
            TargetSelector::Project(crate::projects::ProjectSelector {
                name: "api".into(),
            }),
            vec!["up".into(), "-d".into()],
            ComposeConcurrency::sequential(),
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
    async fn compose_stream_service_appends_service_name() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 0);

        let events = collect_compose_events(compose_stream(
            context(fake_docker_command(&docker)),
            TargetSelector::Service(crate::projects::ServiceSelector {
                project: "api".into(),
                service: "web".into(),
            }),
            vec!["restart".into()],
            ComposeConcurrency::sequential(),
        ))
        .await;

        assert!(events.iter().all(Result::is_ok));
        assert_eq!(
            fs::read_to_string(args_file).unwrap(),
            "compose\n--file\napi.yml\n--project-name\napi\nrestart\nweb\n"
        );
    }

    #[tokio::test]
    async fn compose_stream_service_reports_failure() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 9);

        let events = collect_compose_events(compose_stream(
            context(fake_docker_command(&docker)),
            TargetSelector::Service(crate::projects::ServiceSelector {
                project: "api".into(),
                service: "web".into(),
            }),
            vec!["restart".into()],
            ComposeConcurrency::sequential(),
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
    async fn compose_stream_all_emits_project_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 0);

        let events = collect_compose_events(compose_stream(
            context(fake_docker_command(&docker)),
            TargetSelector::All,
            vec!["pull".into()],
            ComposeConcurrency::sequential(),
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
    async fn compose_stream_parallel_all_emits_project_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 0);

        let events = collect_compose_events(compose_stream(
            context(fake_docker_command(&docker)),
            TargetSelector::All,
            vec!["pull".into()],
            ComposeConcurrency::unbounded(),
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
    async fn compose_stream_all_sequential_finishes_each_project_before_next() {
        let dir = tempfile::tempdir().unwrap();
        let log_file = dir.path().join("timing-log");
        let docker = write_timed_fake_docker(dir.path(), &log_file);

        let events = collect_compose_events(compose_stream(
            context(fake_docker_command(&docker)),
            TargetSelector::All,
            vec!["up".into()],
            ComposeConcurrency::sequential(),
        ))
        .await;

        assert!(events.iter().all(Result::is_ok));
        assert_eq!(
            fs::read_to_string(log_file).unwrap(),
            "start api\nend api\nstart worker\nend worker\n"
        );
    }

    #[tokio::test]
    async fn compose_stream_all_parallel_starts_projects_before_finishing() {
        let dir = tempfile::tempdir().unwrap();
        let log_file = dir.path().join("timing-log");
        let docker = write_timed_fake_docker(dir.path(), &log_file);

        let events = collect_compose_events(compose_stream(
            context(fake_docker_command(&docker)),
            TargetSelector::All,
            vec!["up".into()],
            ComposeConcurrency::unbounded(),
        ))
        .await;

        assert!(events.iter().all(Result::is_ok));
        let log = fs::read_to_string(log_file).unwrap();
        let lines = log.lines().collect::<Vec<_>>();

        assert_eq!(lines.len(), 4);
        assert!(lines[..2].contains(&"start api"));
        assert!(lines[..2].contains(&"start worker"));
        assert!(lines[2..].contains(&"end api"));
        assert!(lines[2..].contains(&"end worker"));
    }

    #[tokio::test]
    async fn compose_stream_project_reports_failure() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 2);

        let events = collect_compose_events(compose_stream(
            context(fake_docker_command(&docker)),
            TargetSelector::Project(crate::projects::ProjectSelector {
                name: "api".into(),
            }),
            vec!["up".into()],
            ComposeConcurrency::sequential(),
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
    async fn compose_stream_all_collects_failures_and_continues() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 5);

        let events = collect_compose_events(compose_stream(
            context(fake_docker_command(&docker)),
            TargetSelector::All,
            vec!["up".into()],
            ComposeConcurrency::sequential(),
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
    async fn compose_stream_parallel_all_collects_failures() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 5);

        let events = collect_compose_events(compose_stream(
            context(fake_docker_command(&docker)),
            TargetSelector::All,
            vec!["up".into()],
            ComposeConcurrency::unbounded(),
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
