use paste::paste;

use clap::{Args, Subcommand};
use nirion_lib::context::NirionContext;
use std::num::NonZeroUsize;
use tokio::time::Duration;

use crate::progress::{LifecycleOptions, lifecycle_options};

#[derive(Args, Debug, Clone)]
pub struct LifecycleArgs {
    /// Use plain Docker Compose output instead of the progress UI
    #[arg(long)]
    pub plain: bool,

    /// Refresh interval in seconds for status updates when monitoring
    #[arg(short = 'r', long, default_value = "250ms", value_parser = humantime::parse_duration)]
    pub refresh: Duration,

    /// Suppress non-essential output
    #[arg(short, long)]
    pub quiet: bool,

    /// Maximum number of projects to run concurrently
    #[arg(short = 'j', long)]
    pub jobs: Option<NonZeroUsize>,
}

impl LifecycleArgs {
    pub fn options(
        &self,
        wait_for_healthchecks: bool,
    ) -> LifecycleOptions {
        lifecycle_options(
            self.plain,
            self.quiet,
            self.jobs,
            self.refresh,
            wait_for_healthchecks,
        )
    }
}

macro_rules! define_commands {
    (
        [ $( $modname:ident ),* $(,)? ]
    ) => {
        paste! {
            $(
                pub mod $modname;
                use crate::commands::$modname::{ [<handle_ $modname>], [<$modname:camel Args>] };
            )*

            #[derive(Subcommand)]
            pub enum Commands {
                $(
                    [<$modname:camel>] {
                        #[command(flatten)]
                        args: [<$modname:camel Args>],
                    },
                )*
            }

            pub async fn handle_command(
                command: &Commands,
                context: &NirionContext
            ) -> anyhow::Result<()> {
                match command {
                    $(
                        Commands::[<$modname:camel>] { args } =>
                            [<handle_ $modname>](args, context).await?,
                    )*
                }
                Ok(())
            }
        }
    };
}

define_commands!([
    up,
    down,
    reload,
    start,
    stop,
    list,
    pull,
    update,
    lock,
    exec,
    logs,
    cat,
    ps,
    top,
    volumes,
    restart,
    compose_exec,
    monitor,
    patch,
    inspect
]);
