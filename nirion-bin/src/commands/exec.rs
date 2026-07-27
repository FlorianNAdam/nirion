use clap::{Args, ValueHint};
use nirion_lib::{
    context::NirionContext,
    exec::{exec, ExecIo, ExecOutput, ExecRequest, ExecTerminalSize},
};
use rustix::fs::{fcntl_getfl, fcntl_setfl, OFlags};
use rustix::termios::{
    isatty, tcgetattr, tcgetwinsize, tcsetattr, OptionalActions,
};
use std::{
    io::Read,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
    task::{spawn_blocking, JoinHandle},
};

use crate::{ClapSelector, ServiceSelector};

/// Execute a command in a running service container
#[derive(Args, Debug, Clone)]
pub struct ExecArgs {
    /// Service selector: project.service
    #[arg(
        default_value = "*",
        value_parser = ServiceSelector::clap_parse,
        add = ServiceSelector::clap_completer()
    )]
    target: ServiceSelector,

    /// Detached mode: run in background
    #[arg(short = 'd', long)]
    detach: bool,

    /// Disable pseudo-TTY allocation
    #[arg(short = 'T', long)]
    no_tty: bool,

    /// Run as this user
    #[arg(short = 'u', long)]
    user: Option<String>,

    /// Set working directory inside container
    #[arg(short = 'w', long, value_hint = ValueHint::DirPath)]
    workdir: Option<String>,

    /// Container index if service has multiple replicas
    #[arg(long)]
    index: Option<u32>,

    /// Environment variables (can be repeated)
    #[arg(short = 'e', long)]
    env: Vec<String>,

    /// Privileged mode
    #[arg(long)]
    privileged: bool,

    /// Command to execute in container
    cmd: Vec<String>,
}

pub async fn handle_exec(
    args: &ExecArgs,
    context: &NirionContext,
) -> anyhow::Result<()> {
    let mut request = args.request();

    let interactive =
        !request.detach && !request.no_tty && isatty(std::io::stdin());
    if !request.detach && !interactive {
        request.no_tty = true;
    }
    let raw_mode = if interactive {
        Some(RawMode::enable()?)
    } else {
        None
    };

    let (input_tx, input) = mpsc::unbounded_channel();
    let (output, output_rx) = mpsc::unbounded_channel();
    let stdin_done = Arc::new(AtomicBool::new(false));
    let stdin_thread = spawn_interactive_stdin_forwarder(
        interactive,
        input_tx.clone(),
        stdin_done.clone(),
    );
    let stdin_task =
        spawn_stdin_forwarder(request.detach || interactive, input_tx);
    let output_task = spawn_output_forwarder(output_rx);

    let terminal_size = interactive
        .then(current_terminal_size)
        .flatten();

    let result = exec(
        context,
        &request,
        ExecIo {
            input,
            output,
            terminal_size,
        },
    )
    .await;
    stdin_done.store(true, Ordering::Relaxed);
    drop(raw_mode);
    if let Some(stdin_task) = stdin_task {
        stdin_task.abort();
    }
    if let Some(stdin_thread) = stdin_thread {
        stdin_thread.await??;
    }
    output_task.await??;
    result
}

fn spawn_interactive_stdin_forwarder(
    interactive: bool,
    input_tx: mpsc::UnboundedSender<Vec<u8>>,
    done: Arc<AtomicBool>,
) -> Option<JoinHandle<anyhow::Result<()>>> {
    interactive
        .then(|| spawn_blocking(move || read_interactive_stdin(input_tx, done)))
}

fn spawn_stdin_forwarder(
    disabled: bool,
    input_tx: mpsc::UnboundedSender<Vec<u8>>,
) -> Option<JoinHandle<anyhow::Result<()>>> {
    (!disabled).then(|| {
        tokio::spawn(async move {
            let mut stdin = io::stdin();
            let mut buffer = [0; 8192];
            loop {
                let n = stdin.read(&mut buffer).await?;
                if n == 0 {
                    break;
                }
                if input_tx
                    .send(buffer[..n].to_vec())
                    .is_err()
                {
                    return anyhow::Ok(());
                }
            }
            anyhow::Ok(())
        })
    })
}

fn spawn_output_forwarder(
    mut output_rx: mpsc::UnboundedReceiver<ExecOutput>
) -> JoinHandle<anyhow::Result<()>> {
    tokio::spawn(async move {
        let mut stdout = io::stdout();
        let mut stderr = io::stderr();
        while let Some(event) = output_rx.recv().await {
            match event {
                ExecOutput::Stdout(data) => {
                    stdout.write_all(&data).await?;
                    stdout.flush().await?;
                }
                ExecOutput::Stderr(data) => {
                    stderr.write_all(&data).await?;
                    stderr.flush().await?;
                }
            }
        }
        anyhow::Ok(())
    })
}

impl ExecArgs {
    fn request(&self) -> ExecRequest {
        ExecRequest {
            target: self.target.clone(),
            detach: self.detach,
            no_tty: self.no_tty,
            user: self.user.clone(),
            workdir: self.workdir.clone(),
            index: self.index,
            env: self.env.clone(),
            privileged: self.privileged,
            cmd: self.cmd.clone(),
        }
    }
}

fn current_terminal_size() -> Option<ExecTerminalSize> {
    tcgetwinsize(std::io::stdout())
        .ok()
        .map(|size| ExecTerminalSize {
            rows: size.ws_row,
            cols: size.ws_col,
            xpixel: size.ws_xpixel,
            ypixel: size.ws_ypixel,
        })
}

struct RawMode {
    original: rustix::termios::Termios,
    original_flags: OFlags,
}

impl RawMode {
    fn enable() -> anyhow::Result<Self> {
        let original = tcgetattr(std::io::stdin())?;
        let original_flags = fcntl_getfl(std::io::stdin())?;
        let mut raw = original.clone();
        raw.make_raw();
        tcsetattr(std::io::stdin(), OptionalActions::Now, &raw)?;
        fcntl_setfl(std::io::stdin(), original_flags | OFlags::NONBLOCK)?;
        Ok(Self {
            original,
            original_flags,
        })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ =
            tcsetattr(std::io::stdin(), OptionalActions::Now, &self.original);
        let _ = fcntl_setfl(std::io::stdin(), self.original_flags);
    }
}

fn read_interactive_stdin(
    input_tx: mpsc::UnboundedSender<Vec<u8>>,
    done: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let mut stdin = std::io::stdin().lock();
    let mut buffer = [0; 8192];

    while !done.load(Ordering::Relaxed) {
        match stdin.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                if input_tx
                    .send(buffer[..n].to_vec())
                    .is_err()
                {
                    return Ok(());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error.into()),
        }
    }

    Ok(())
}
