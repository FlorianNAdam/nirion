use std::{
    fs::File,
    io::{Read, Write},
    ops::Deref,
    os::fd::OwnedFd,
    process::Stdio,
};

use anyhow::Context;
use rustix::{
    fs::{OFlags, fcntl_getfl, fcntl_setfl},
    io::dup,
    pty::{grantpt, ioctl_tiocgptpeer, openpt, unlockpt, OpenptFlags},
    termios::{
        tcsetwinsize, OptionalActions, Winsize, },
};
use tokio::{
    io::unix::AsyncFd,
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    sync::{mpsc, watch},
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
    Resize(ExecTerminalSize),
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
    io: ExecIo,
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
    let output = io.output.clone();

    let stdin_task =
        spawn_pipe_input_task(&mut child, request.detach, io.input)?;
    let stdout_task = spawn_pipe_output_task(
        child.stdout.take(),
        ExecOutput::Stdout,
        output.clone(),
        "stdout",
    )?;
    let stderr_task = spawn_pipe_output_task(
        child.stderr.take(),
        ExecOutput::Stderr,
        output,
        "stderr",
    )?;

    let status = child.wait().await?;

    if let Some(stdin_task) = stdin_task {
        stdin_task.abort();
    }
    stdout_task.await??;
    stderr_task.await??;

    ensure_exec_success(project_name, service_name, status)
}

fn spawn_pipe_input_task(
    child: &mut tokio::process::Child,
    detach: bool,
    mut input: mpsc::UnboundedReceiver<ExecInput>,
) -> anyhow::Result<Option<tokio::task::JoinHandle<anyhow::Result<()>>>> {
    if detach {
        return Ok(None);
    }

    let Some(mut child_stdin) = child.stdin.take() else {
        anyhow::bail!("failed to capture docker compose exec stdin");
    };

    Ok(Some(tokio::spawn(async move {
        while let Some(input) = input.recv().await {
            if let ExecInput::Stdin(data) = input {
                child_stdin.write_all(&data).await?;
            }
        }
        let _ = child_stdin.shutdown().await;
        Ok(())
    })))
}

fn spawn_pipe_output_task<R>(
    output: Option<R>,
    event: fn(Vec<u8>) -> ExecOutput,
    tx: mpsc::UnboundedSender<ExecOutput>,
    name: &str,
) -> anyhow::Result<tokio::task::JoinHandle<anyhow::Result<()>>>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let Some(output) = output else {
        anyhow::bail!("failed to capture docker compose exec {name}");
    };
    Ok(tokio::spawn(read_exec_output(output, event, tx)))
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

    let mut child = command
        .spawn()
        .context("failed to execute docker compose exec")?;

    let master_in = File::from(dup(&pty.master)?);
    let master_out = File::from(pty.master);
    set_nonblocking(&master_in)?;
    set_nonblocking(&master_out)?;
    let (child_done_tx, child_done_rx) = watch::channel(false);
    let stdin_task = tokio::spawn(copy_channel_to_pty(
        io.input,
        AsyncFd::new(master_in)?,
        child_done_rx.clone(),
    ));
    let stdout_task = tokio::spawn(copy_pty_to_channel(
        AsyncFd::new(master_out)?,
        io.output,
        child_done_rx,
    ));

    let status = child.wait().await?;
    let _ = child_done_tx.send(true);
    stdin_task.await??;
    stdout_task.await??;

    ensure_exec_success(project_name, service_name, status)
}

fn ensure_exec_success(
    project_name: &str,
    service_name: &str,
    status: std::process::ExitStatus,
) -> anyhow::Result<()> {
    if status.success() {
        return Ok(());
    }

    anyhow::bail!(
        "Command failed in {}.{} with status {}",
        project_name,
        service_name,
        status
    );
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

async fn copy_channel_to_pty(
    mut input: mpsc::UnboundedReceiver<ExecInput>,
    output: AsyncFd<File>,
    mut child_done: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    loop {
        let input = tokio::select! {
            input = input.recv() => input,
            changed = child_done.changed(), if !*child_done.borrow() => {
                changed?;
                if *child_done.borrow() {
                    break;
                }
                continue;
            }
        };

        let Some(input) = input else { break };

        match input {
            ExecInput::Stdin(data) => {
                write_all_pty(&output, &data).await?;
            }
            ExecInput::Resize(size) => {
                let _ = tcsetwinsize(output.get_ref(), size.into());
            }
        }
    }
    Ok(())
}

async fn copy_pty_to_channel(
    input: AsyncFd<File>,
    output: mpsc::UnboundedSender<ExecOutput>,
    mut child_done: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let mut buffer = [0; 8192];
    loop {
        tokio::select! {
            biased;

            changed = child_done.changed(), if !*child_done.borrow() => {
                changed?;
                if *child_done.borrow() {
                    drain_pty(&input, &output, &mut buffer)?;
                    break;
                }
            }

            ready = input.readable() => {
                let mut guard = ready?;
                match guard.try_io(|inner| {
                    let mut input = inner.get_ref();
                    input.read(&mut buffer)
                }) {
                    Ok(Ok(0)) => break,
                    Ok(Ok(n)) => {
                        let _ = output.send(ExecOutput::Stdout(buffer[..n].to_vec()));
                    }
                    Ok(Err(error)) if is_linux_pty_hangup(&error) => break,
                    Ok(Err(error)) => return Err(error.into()),
                    Err(_would_block) => continue,
                }
            }
        }
    }
    Ok(())
}

async fn write_all_pty(
    output: &AsyncFd<File>,
    mut data: &[u8],
) -> anyhow::Result<()> {
    while !data.is_empty() {
        let mut guard = output.writable().await?;
        match guard.try_io(|inner| {
            let mut output = inner.get_ref();
            output.write(data)
        }) {
            Ok(Ok(0)) => {
                anyhow::bail!("failed to write to docker compose exec pty")
            }
            Ok(Ok(n)) => data = &data[n..],
            Ok(Err(error)) => return Err(error.into()),
            Err(_would_block) => continue,
        }
    }
    Ok(())
}

fn drain_pty(
    input: &AsyncFd<File>,
    output: &mpsc::UnboundedSender<ExecOutput>,
    buffer: &mut [u8],
) -> anyhow::Result<()> {
    loop {
        let mut input = input.get_ref();
        match input.read(buffer) {
            Ok(0) => break,
            Ok(n) => {
                let _ = output.send(ExecOutput::Stdout(buffer[..n].to_vec()));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                break;
            }
            Err(error) if is_linux_pty_hangup(&error) => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn is_linux_pty_hangup(error: &std::io::Error) -> bool {
    const LINUX_EIO: i32 = 5;

    // Linux reports EIO when the slave side of a PTY has closed. Treat that as
    // EOF for the master-side reader.
    error.raw_os_error() == Some(LINUX_EIO)
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
    use rustix::termios::tcgetwinsize;
    use std::{fs, io::Write, os::unix::fs::PermissionsExt, path::Path};
    use std::{path::PathBuf, sync::Arc};
    use tokio::time::{Duration as TokioDuration, timeout};

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

    fn write_fake_docker_script(
        dir: &Path,
        script: &str,
    ) -> String {
        let docker = dir.join("docker");
        let tmp = dir.join("docker.tmp");
        let mut file = fs::File::create(&tmp).unwrap();
        file.write_all(script.as_bytes())
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

    fn exec_io_with_channels() -> (
        mpsc::UnboundedSender<ExecInput>,
        mpsc::UnboundedReceiver<ExecOutput>,
        ExecIo,
    ) {
        let (input_tx, input) = mpsc::unbounded_channel();
        let (output, output_rx) = mpsc::unbounded_channel();
        (
            input_tx,
            output_rx,
            ExecIo {
                input,
                output,
                terminal_size: None,
            },
        )
    }

    fn drain_outputs(
        output_rx: &mut mpsc::UnboundedReceiver<ExecOutput>
    ) -> Vec<ExecOutput> {
        let mut outputs = Vec::new();
        while let Ok(output) = output_rx.try_recv() {
            outputs.push(output);
        }
        outputs
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

    #[tokio::test]
    async fn exec_detached_uses_null_stdin_and_no_input_task() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker(dir.path(), &args_file, 0);
        let mut request = request(vec!["sleep", "10"]);
        request.detach = true;

        exec(&context(fake_docker_command(&docker)), &request, exec_io())
            .await
            .unwrap();

        assert_eq!(
            fs::read_to_string(args_file).unwrap(),
            "compose\n--file\ncompose.yml\n--project-name\nmyapp\nexec\n-d\nweb\nsleep\n10\n"
        );
    }

    #[tokio::test]
    async fn spawn_pipe_input_task_reports_missing_stdin() {
        let mut child = tokio::process::Command::new("/bin/sh")
            .arg("-c")
            .arg("true")
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let (_, input) = mpsc::unbounded_channel();

        let err = spawn_pipe_input_task(&mut child, false, input).unwrap_err();

        assert_eq!(
            err.to_string(),
            "failed to capture docker compose exec stdin"
        );
        child.wait().await.unwrap();
    }

    #[tokio::test]
    async fn spawn_pipe_input_task_returns_none_for_detached_exec() {
        let mut child = tokio::process::Command::new("/bin/sh")
            .arg("-c")
            .arg("true")
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let (_, input) = mpsc::unbounded_channel();

        let task = spawn_pipe_input_task(&mut child, true, input).unwrap();

        assert!(task.is_none());
        child.wait().await.unwrap();
    }

    #[test]
    fn spawn_pipe_output_task_reports_missing_output() {
        let (output, _) = mpsc::unbounded_channel();

        let err = spawn_pipe_output_task::<tokio::process::ChildStdout>(
            None,
            ExecOutput::Stdout,
            output,
            "stdout",
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "failed to capture docker compose exec stdout"
        );
    }

    #[tokio::test]
    async fn read_exec_output_handles_empty_stream() {
        let (output, mut output_rx) = mpsc::unbounded_channel();

        read_exec_output(tokio::io::empty(), ExecOutput::Stdout, output)
            .await
            .unwrap();

        assert!(output_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn exec_with_pipes_forwards_stdin_and_closes_on_eof() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let stdin_file = dir.path().join("stdin");
        let docker = write_fake_docker_script(
            dir.path(),
            &format!(
                r#"#!/bin/sh
printf '%s\n' "$@" > '{}'
cat > '{}'
"#,
                args_file.display(),
                stdin_file.display(),
            ),
        );
        let (input_tx, _output_rx, io) = exec_io_with_channels();

        input_tx
            .send(ExecInput::Stdin(b"hello\nfrom stdin".to_vec()))
            .unwrap();
        drop(input_tx);

        timeout(
            TokioDuration::from_secs(1),
            exec(
                &context(fake_docker_command(&docker)),
                &no_tty_request(vec!["cat"]),
                io,
            ),
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(
            fs::read_to_string(stdin_file).unwrap(),
            "hello\nfrom stdin"
        );
        assert_eq!(
            fs::read_to_string(args_file).unwrap(),
            "compose\n--file\ncompose.yml\n--project-name\nmyapp\nexec\n-T\nweb\ncat\n"
        );
    }

    #[tokio::test]
    async fn exec_with_pipes_forwards_stdout_and_stderr() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker_script(
            dir.path(),
            &format!(
                r#"#!/bin/sh
printf '%s\n' "$@" > '{}'
printf stdout-message
printf stderr-message >&2
"#,
                args_file.display(),
            ),
        );
        let (input_tx, mut output_rx, io) = exec_io_with_channels();
        drop(input_tx);

        exec(
            &context(fake_docker_command(&docker)),
            &no_tty_request(vec!["print"]),
            io,
        )
        .await
        .unwrap();

        let outputs = drain_outputs(&mut output_rx);
        assert!(
            outputs.contains(&ExecOutput::Stdout(b"stdout-message".to_vec()))
        );
        assert!(
            outputs.contains(&ExecOutput::Stderr(b"stderr-message".to_vec()))
        );
    }

    #[tokio::test]
    async fn exec_with_pty_forwards_stdin_and_stdout() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let docker = write_fake_docker_script(
            dir.path(),
            &format!(
                r#"#!/bin/sh
printf '%s\n' "$@" > '{}'
IFS= read -r line
printf 'reply:%s\n' "$line"
"#,
                args_file.display(),
            ),
        );
        let (input_tx, mut output_rx, io) = exec_io_with_channels();

        input_tx
            .send(ExecInput::Stdin(b"hello pty\n".to_vec()))
            .unwrap();
        drop(input_tx);

        timeout(
            TokioDuration::from_secs(1),
            exec(
                &context(fake_docker_command(&docker)),
                &request(vec!["sh"]),
                io,
            ),
        )
        .await
        .unwrap()
        .unwrap();

        let output = drain_outputs(&mut output_rx)
            .into_iter()
            .filter_map(|output| match output {
                ExecOutput::Stdout(data) => Some(data),
                ExecOutput::Stderr(_) => None,
            })
            .flatten()
            .collect::<Vec<_>>();
        let output = String::from_utf8_lossy(&output);
        assert!(output.contains("reply:hello pty"), "output: {output:?}");
        assert_eq!(
            fs::read_to_string(args_file).unwrap(),
            "compose\n--file\ncompose.yml\n--project-name\nmyapp\nexec\nweb\nsh\n"
        );
    }

    #[tokio::test]
    async fn copy_channel_to_pty_stops_when_child_is_done() {
        let pty = open_pty().unwrap();
        let master = File::from(pty.master);
        let _slave = File::from(pty.slave);
        let (_tx, rx) = mpsc::unbounded_channel();
        let (child_done_tx, child_done_rx) = watch::channel(false);

        tokio::spawn(async move {
            tokio::task::yield_now().await;
            child_done_tx.send(true).unwrap();
        });

        timeout(
            TokioDuration::from_secs(1),
            copy_channel_to_pty(
                rx,
                AsyncFd::new(master).unwrap(),
                child_done_rx,
            ),
        )
        .await
        .unwrap()
        .unwrap();
    }

    #[tokio::test]
    async fn drain_pty_forwards_available_output() {
        let pty = open_pty().unwrap();
        let master = File::from(pty.master);
        let mut slave = File::from(pty.slave);
        let (output, mut output_rx) = mpsc::unbounded_channel();
        let mut buffer = [0; 8192];

        set_nonblocking(&master).unwrap();
        slave
            .write_all(b"drained from pty\n")
            .unwrap();

        drain_pty(&AsyncFd::new(master).unwrap(), &output, &mut buffer)
            .unwrap();

        assert_eq!(
            output_rx.try_recv().unwrap(),
            ExecOutput::Stdout(b"drained from pty\r\n".to_vec())
        );
    }

    #[tokio::test]
    async fn copy_pty_to_channel_forwards_slave_output() {
        let pty = open_pty().unwrap();
        let mut slave = File::from(pty.slave);
        let master = File::from(pty.master);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (child_done_tx, child_done_rx) = watch::channel(false);

        set_nonblocking(&master).unwrap();
        let task = tokio::spawn(copy_pty_to_channel(
            AsyncFd::new(master).unwrap(),
            tx,
            child_done_rx,
        ));
        slave
            .write_all(b"hello from pty")
            .unwrap();

        let output = timeout(TokioDuration::from_secs(1), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(output, ExecOutput::Stdout(b"hello from pty".to_vec()));

        let _ = child_done_tx.send(true);
        drop(slave);
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn copy_channel_to_pty_applies_resize_events() {
        let pty = open_pty().unwrap();
        let master_for_assertion = File::from(dup(&pty.master).unwrap());
        let _slave = File::from(pty.slave);
        let master = File::from(pty.master);
        let (tx, rx) = mpsc::unbounded_channel();
        let (_child_done_tx, child_done_rx) = watch::channel(false);
        let size = ExecTerminalSize {
            rows: 37,
            cols: 103,
            xpixel: 7,
            ypixel: 11,
        };

        tx.send(ExecInput::Resize(size))
            .unwrap();
        drop(tx);

        set_nonblocking(&master).unwrap();
        copy_channel_to_pty(rx, AsyncFd::new(master).unwrap(), child_done_rx)
            .await
            .unwrap();

        let actual = tcgetwinsize(&master_for_assertion).unwrap();
        assert_eq!(actual.ws_row, size.rows);
        assert_eq!(actual.ws_col, size.cols);
        assert_eq!(actual.ws_xpixel, size.xpixel);
        assert_eq!(actual.ws_ypixel, size.ypixel);
    }
}
