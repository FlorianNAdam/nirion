use std::ops::Deref;

use anyhow::Context;

use crate::{
    context::NirionContext,
    projects::{Projects, ServiceSelector},
};

#[derive(Debug, Clone)]
pub struct ExecRequest {
    pub target: ServiceSelector,
    pub detach: bool,
    pub no_tty: bool,
    pub user: Option<String>,
    pub workdir: Option<String>,
    pub index: Option<u32>,
    pub env: Vec<String>,
    pub privileged: bool,
    pub cmd: Vec<String>,
}

pub fn exec(
    context: &NirionContext,
    request: &ExecRequest,
) -> anyhow::Result<()> {
    let project_name = &request.target.project;
    let service_name = &request.target.service;
    let cmd_args = build_exec_args(&context.projects, request)?;

    let status = context
        .docker_command
        .std_command()
        .arg("compose")
        .args(&cmd_args)
        .status()
        .context("failed to execute docker compose exec")?;

    if !status.success() {
        anyhow::bail!(
            "Command failed in {}.{} with status {}",
            project_name,
            service_name,
            status
        );
    }

    Ok(())
}

fn build_exec_args(
    projects: &Projects,
    request: &ExecRequest,
) -> anyhow::Result<Vec<String>> {
    if request.cmd.is_empty() {
        anyhow::bail!("No command specified for exec");
    }

    let mut common_args = vec![];
    if request.detach {
        common_args.push("-d".to_string());
    }
    if request.no_tty {
        common_args.push("-T".to_string());
    }
    if let Some(user) = &request.user {
        common_args.push("-u".to_string());
        common_args.push(user.clone());
    }
    if let Some(workdir) = &request.workdir {
        common_args.push("-w".to_string());
        common_args.push(workdir.clone());
    }
    if let Some(idx) = request.index {
        common_args.push("--index".to_string());
        common_args.push(idx.to_string());
    }
    for e in &request.env {
        common_args.push("-e".to_string());
        common_args.push(e.clone());
    }
    if request.privileged {
        common_args.push("--privileged".to_string());
    }

    let project_name = &request.target.project;
    let service_name = &request.target.service;

    let project = &projects[project_name];
    let mut cmd_args = vec![
        "--file".to_string(),
        project.docker_compose.clone(),
        "--project-name".to_string(),
        project.name.deref().to_string(),
        "exec".to_string(),
    ];
    cmd_args.extend(common_args);
    cmd_args.push(service_name.clone());
    cmd_args.extend(request.cmd.clone());

    Ok(cmd_args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{docker::DockerCommand, lock::LockedImages};
    use nirion_oci_lib::client::NirionOciClient;
    use std::{fs, os::unix::fs::PermissionsExt, path::Path};
    use std::{path::PathBuf, sync::Arc};

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

    fn projects() -> Projects {
        serde_json::from_value(serde_json::json!({
            "myapp": {
                "name": "myapp",
                "dockerCompose": "compose.yml",
                "services": {
                    "web": {
                        "image": "nginx",
                        "resolvedImage": "nginx@sha256:abc",
                        "healthcheck": false,
                        "restart": null
                    }
                }
            }
        }))
        .unwrap()
    }

    fn request(cmd: Vec<&str>) -> ExecRequest {
        ExecRequest {
            target: ServiceSelector {
                project: "myapp".into(),
                service: "web".into(),
            },
            detach: false,
            no_tty: false,
            user: None,
            workdir: None,
            index: None,
            env: Vec::new(),
            privileged: false,
            cmd: cmd
                .into_iter()
                .map(str::to_string)
                .collect(),
        }
    }

    #[test]
    fn build_exec_args_rejects_empty_command() {
        let projects = projects();
        let err = build_exec_args(&projects, &request(Vec::new())).unwrap_err();

        assert_eq!(err.to_string(), "No command specified for exec");
    }

    #[test]
    fn build_exec_args_builds_minimal_command() {
        let projects = projects();
        let args =
            build_exec_args(&projects, &request(vec!["sh", "-c", "uptime"]))
                .unwrap();

        assert_eq!(
            args,
            vec![
                "--file",
                "compose.yml",
                "--project-name",
                "myapp",
                "exec",
                "web",
                "sh",
                "-c",
                "uptime"
            ]
        );
    }

    #[test]
    fn build_exec_args_includes_all_options_in_order() {
        let projects = projects();
        let mut req = request(vec!["printenv"]);
        req.detach = true;
        req.no_tty = true;
        req.user = Some("1000:1000".into());
        req.workdir = Some("/srv".into());
        req.index = Some(2);
        req.env = vec!["FOO=bar".into(), "BAZ=qux".into()];
        req.privileged = true;

        let args = build_exec_args(&projects, &req).unwrap();

        assert_eq!(
            args,
            vec![
                "--file",
                "compose.yml",
                "--project-name",
                "myapp",
                "exec",
                "-d",
                "-T",
                "-u",
                "1000:1000",
                "-w",
                "/srv",
                "--index",
                "2",
                "-e",
                "FOO=bar",
                "-e",
                "BAZ=qux",
                "--privileged",
                "web",
                "printenv"
            ]
        );
    }

    #[test]
    fn exec_runs_docker_compose_exec() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 0);

        exec(
            &context(fake_docker_command(&docker)),
            &request(vec!["true"]),
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(args_file).unwrap(),
            "compose\n--file\ncompose.yml\n--project-name\nmyapp\nexec\nweb\ntrue\n"
        );
    }

    #[test]
    fn exec_reports_failed_status() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 7);

        let err = exec(
            &context(fake_docker_command(&docker)),
            &request(vec!["false"]),
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("Command failed in myapp.web with status")
        );
    }

    #[test]
    fn exec_reports_spawn_failure() {
        let dir = tempfile::tempdir().unwrap();
        let missing_docker = dir.path().join("missing-docker");

        let err = exec(
            &context(DockerCommand::new(missing_docker)),
            &request(vec!["true"]),
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("failed to execute docker compose exec")
        );
    }
}
