use std::{
    fs::File,
    io::{Read, Write},
    ops::Deref,
    os::fd::OwnedFd,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use anyhow::Context;
use rustix::{
    fs::{OFlags, fcntl_getfl, fcntl_setfl},
    io::dup,
    process::{Pid, Signal, ioctl_tiocsctty, kill_process_group, setsid},
    pty::{grantpt, ioctl_tiocgptpeer, openpt, unlockpt, OpenptFlags},
    termios::{
        tcsetwinsize, OptionalActions, Winsize, },
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
};

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

pub struct ExecIo {
    pub input: mpsc::UnboundedReceiver<ExecInput>,
    pub output: mpsc::UnboundedSender<ExecOutput>,
    pub terminal_size: Option<ExecTerminalSize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecInput {
    Stdin(Vec<u8>),
    Interrupt,
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecOutput {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecTerminalSize {
    pub rows: u16,
    pub cols: u16,
    pub xpixel: u16,
    pub ypixel: u16,
}

pub async fn exec(
    context: &NirionContext,
    request: &ExecRequest,
    io: ExecIo,
) -> anyhow::Result<()> {
    if request.detach || request.no_tty {
        return exec_with_pipes(context, request, io).await;
    }
    exec_with_pty(context, request, io).await
}

async fn exec_with_pipes(
    context: &NirionContext,
    request: &ExecRequest,
    mut io: ExecIo,
) -> anyhow::Result<()> {
    let project_name = &request.target.project;
    let service_name = &request.target.service;
    let cmd_args = build_exec_args(&context.projects, request)?;

    let mut command = context.docker_command.command();
    command.arg("compose").args(&cmd_args);
    if request.detach {
        command.stdin(Stdio::null());
    } else {
        command.stdin(Stdio::piped());
    }
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .context("failed to execute docker compose exec")?;
    let child_pgid = child_process_group(&child)?;

    let stdin_task = if request.detach {
        None
    } else {
        let Some(mut child_stdin) = child.stdin.take() else {
            anyhow::bail!("failed to capture docker compose exec stdin");
        };
        Some(tokio::spawn(async move {
            while let Some(input) = io.input.recv().await {
                match input {
                    ExecInput::Stdin(data) => {
                        child_stdin.write_all(&data).await?;
                    }
                    ExecInput::Interrupt => {
                        signal_interrupt(child_pgid);
                    }
                    ExecInput::Eof => break,
                }
            }
            let _ = child_stdin.shutdown().await;
            anyhow::Ok(())
        }))
    };

    let Some(child_stdout) = child.stdout.take() else {
        anyhow::bail!("failed to capture docker compose exec stdout");
    };
    let stdout_task = tokio::spawn(read_exec_output(
        child_stdout,
        ExecOutput::Stdout,
        io.output.clone(),
    ));

    let Some(child_stderr) = child.stderr.take() else {
        anyhow::bail!("failed to capture docker compose exec stderr");
    };
    let stderr_task = tokio::spawn(read_exec_output(
        child_stderr,
        ExecOutput::Stderr,
        io.output.clone(),
    ));

    let status = child.wait().await?;

    if let Some(stdin_task) = stdin_task {
        stdin_task.abort();
    }
    stdout_task.await??;
    stderr_task.await??;

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

async fn exec_with_pty(
    context: &NirionContext,
    request: &ExecRequest,
    io: ExecIo,
) -> anyhow::Result<()> {
    let project_name = &request.target.project;
    let service_name = &request.target.service;
    let cmd_args = build_exec_args(&context.projects, request)?;
    let terminal_size = io.terminal_size;
    let pty = open_pty()?;

    if let Some(size) = terminal_size {
        let _ = tcsetwinsize(&pty.slave, size.into());
    }

    let controlling_terminal = dup(&pty.slave)?;
    let stdin = Stdio::from(dup(&pty.slave)?);
    let stdout = Stdio::from(dup(&pty.slave)?);
    let stderr = Stdio::from(pty.slave);

    let mut command = context.docker_command.command();
    command
        .arg("compose")
        .args(&cmd_args)
        .stdin(stdin)
        .stdout(stdout)
        .stderr(stderr);

    #[cfg(unix)]
    unsafe {
        command.pre_exec(move || {
            setsid().map_err(std::io::Error::from)?;
            ioctl_tiocsctty(&controlling_terminal)
                .map_err(std::io::Error::from)?;
            Ok(())
        });
    }

    let mut child = command
        .spawn()
        .context("failed to execute docker compose exec")?;

    let master_in = File::from(dup(&pty.master)?);
    let master_out = File::from(pty.master);
    let child_done = Arc::new(AtomicBool::new(false));
    let input_child_done = child_done.clone();
    let output_child_done = child_done.clone();
    let stdin_thread = thread::spawn(move || {
        copy_channel_to(io.input, master_in, input_child_done)
    });
    let stdout_thread = thread::spawn(move || {
        copy_pty_to_channel(master_out, io.output, output_child_done)
    });

    let status = child.wait().await?;
    child_done.store(true, Ordering::Relaxed);
    stdin_thread.join().map_err(|_| {
        anyhow::anyhow!("docker compose exec input thread panicked")
    })??;
    stdout_thread.join().map_err(|_| {
        anyhow::anyhow!("docker compose exec output thread panicked")
    })??;

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

struct Pty {
    master: OwnedFd,
    slave: OwnedFd,
}

fn open_pty() -> anyhow::Result<Pty> {
    let flags = OpenptFlags::RDWR | OpenptFlags::NOCTTY | OpenptFlags::CLOEXEC;
    let master = openpt(flags)?;
    grantpt(&master)?;
    unlockpt(&master)?;
    let slave = ioctl_tiocgptpeer(&master, flags)?;
    Ok(Pty { master, slave })
}

fn child_process_group(child: &tokio::process::Child) -> anyhow::Result<Pid> {
    let id = child.id().ok_or_else(|| {
        anyhow::anyhow!("docker compose exec child has no process id")
    })?;
    Pid::from_raw(id as i32).ok_or_else(|| {
        anyhow::anyhow!("docker compose exec child has invalid process id {id}")
    })
}

fn signal_interrupt(pgid: Pid) {
    let _ = kill_process_group(pgid, Signal::INT);
}

async fn read_exec_output(
    mut input: impl AsyncRead + Unpin,
    event: fn(Vec<u8>) -> ExecOutput,
    output: mpsc::UnboundedSender<ExecOutput>,
) -> anyhow::Result<()> {
    let mut buffer = [0; 8192];
    loop {
        let n = input.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        let _ = output.send(event(buffer[..n].to_vec()));
    }
    Ok(())
}

impl From<ExecTerminalSize> for Winsize {
    fn from(size: ExecTerminalSize) -> Self {
        Self {
            ws_row: size.rows,
            ws_col: size.cols,
            ws_xpixel: size.xpixel,
            ws_ypixel: size.ypixel,
        }
    }
}

fn copy_channel_to(
    mut input: mpsc::UnboundedReceiver<ExecInput>,
    mut output: File,
    child_done: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    loop {
        let input = match input.try_recv() {
            Ok(input) => input,
            Err(mpsc::error::TryRecvError::Empty) => {
                if child_done.load(Ordering::Relaxed) {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(mpsc::error::TryRecvError::Disconnected) => break,
        };

        match input {
            ExecInput::Stdin(data) => {
                output.write_all(&data)?;
                output.flush()?;
            }
            ExecInput::Interrupt => {
                output.write_all(&[0x03])?;
                output.flush()?;
            }
            ExecInput::Eof => {
                output.write_all(&[0x04])?;
                output.flush()?;
                break;
            }
        }
    }
    Ok(())
}

fn copy_pty_to_channel(
    mut input: File,
    output: mpsc::UnboundedSender<ExecOutput>,
    child_done: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    set_nonblocking(&input)?;
    let mut buffer = [0; 8192];
    loop {
        match input.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                let _ = output.send(ExecOutput::Stdout(buffer[..n].to_vec()));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if child_done.load(Ordering::Relaxed) {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) if error.raw_os_error() == Some(5) => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn set_nonblocking(file: &File) -> anyhow::Result<()> {
    let flags = fcntl_getfl(file)?;
    fcntl_setfl(file, flags | OFlags::NONBLOCK)?;
    Ok(())
}

fn build_exec_args(
    projects: &Projects,
    request: &ExecRequest,
) -> anyhow::Result<Vec<String>> {
    build_exec_args_with_no_tty(projects, request, request.no_tty)
}

fn build_exec_args_with_no_tty(
    projects: &Projects,
    request: &ExecRequest,
    no_tty: bool,
) -> anyhow::Result<Vec<String>> {
    if request.cmd.is_empty() {
        anyhow::bail!("No command specified for exec");
    }

    let mut common_args = vec![];
    if request.detach {
        common_args.push("-d".to_string());
    }
    if no_tty {
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

    fn no_tty_request(cmd: Vec<&str>) -> ExecRequest {
        let mut request = request(cmd);
        request.no_tty = true;
        request
    }

    fn exec_io() -> ExecIo {
        let (_, input) = mpsc::unbounded_channel();
        let (output, _) = mpsc::unbounded_channel();
        ExecIo {
            input,
            output,
            terminal_size: None,
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

    #[tokio::test]
    async fn exec_runs_docker_compose_exec() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 0);

        exec(
            &context(fake_docker_command(&docker)),
            &no_tty_request(vec!["true"]),
            exec_io(),
        )
        .await
        .unwrap();

        assert_eq!(
            fs::read_to_string(args_file).unwrap(),
            "compose\n--file\ncompose.yml\n--project-name\nmyapp\nexec\n-T\nweb\ntrue\n"
        );
    }

    #[tokio::test]
    async fn exec_reports_failed_status() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 7);

        let err = exec(
            &context(fake_docker_command(&docker)),
            &no_tty_request(vec!["false"]),
            exec_io(),
        )
        .await
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("Command failed in myapp.web with status"));
    }

    #[tokio::test]
    async fn exec_reports_spawn_failure() {
        let dir = tempfile::tempdir().unwrap();
        let missing_docker = dir.path().join("missing-docker");

        let err = exec(
            &context(DockerCommand::new(missing_docker)),
            &no_tty_request(vec!["true"]),
            exec_io(),
        )
        .await
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("failed to execute docker compose exec"));
    }
}
