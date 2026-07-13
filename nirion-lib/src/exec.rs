use std::{ops::Deref, process::Command as ProcCommand};

use anyhow::Context;

use crate::projects::{Projects, ServiceSelector};

#[cfg(test)]
static TEST_DOCKER_CMD: std::sync::Mutex<Option<Vec<String>>> =
    std::sync::Mutex::new(None);

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

pub fn exec(projects: &Projects, request: &ExecRequest) -> anyhow::Result<()> {
    let project_name = &request.target.project;
    let service_name = &request.target.service;
    let cmd_args = build_exec_args(projects, request)?;

    let status = docker_command()
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

fn docker_command() -> ProcCommand {
    #[cfg(test)]
    if let Some(cmd) = TEST_DOCKER_CMD.lock().unwrap().clone() {
        let mut command = ProcCommand::new(&cmd[0]);
        command.args(&cmd[1..]);
        return command;
    }

    ProcCommand::new("docker")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt, path::Path};

    static DOCKER_BIN_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
            "myapp": {
                "name": "myapp",
                "dockerCompose": "compose.yml",
                "services": {
                    "web": {
                        "image": "nginx",
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
        let _docker_bin_lock = DOCKER_BIN_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 0);
        let _docker_bin_guard = DockerBinGuard::set_script(docker);

        exec(&projects(), &request(vec!["true"])).unwrap();

        assert_eq!(
            fs::read_to_string(args_file).unwrap(),
            "compose\n--file\ncompose.yml\n--project-name\nmyapp\nexec\nweb\ntrue\n"
        );
    }

    #[test]
    fn exec_reports_failed_status() {
        let _docker_bin_lock = DOCKER_BIN_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 7);
        let _docker_bin_guard = DockerBinGuard::set_script(docker);

        let err = exec(&projects(), &request(vec!["false"])).unwrap_err();

        assert!(
            err.to_string()
                .contains("Command failed in myapp.web with status")
        );
    }

    #[test]
    fn exec_reports_spawn_failure() {
        let _docker_bin_lock = DOCKER_BIN_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let missing_docker = dir.path().join("missing-docker");
        let _docker_bin_guard = DockerBinGuard::set_command(
            missing_docker
                .to_string_lossy()
                .to_string(),
        );

        let err = exec(&projects(), &request(vec!["true"])).unwrap_err();

        assert!(
            err.to_string()
                .contains("failed to execute docker compose exec")
        );
    }
}
