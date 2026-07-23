use clap::{Args, ValueHint};
use nirion_lib::{
    context::NirionContext,
    exec::{exec, ExecRequest},
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
    exec(
        context,
        &ExecRequest {
            target: args.target.clone(),
            detach: args.detach,
            no_tty: args.no_tty,
            user: args.user.clone(),
            workdir: args.workdir.clone(),
            index: args.index,
            env: args.env.clone(),
            privileged: args.privileged,
            cmd: args.cmd.clone(),
        },
    )
    .await
}
