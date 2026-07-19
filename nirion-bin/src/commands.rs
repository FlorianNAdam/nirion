use paste::paste;

use clap::{Args, Subcommand};
use nirion_lib::context::NirionContext;
use std::num::NonZeroUsize;
use tokio::time::Duration;

use crate::lifecycle::LifecycleOptions;
use crate::progress_render::ProgressPresentation;
use nirion_lib::wait::WaitTarget;

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
        wait: WaitTarget,
    ) -> LifecycleOptions {
        LifecycleOptions {
            presentation: self.presentation(),
            jobs: self.jobs(),
            refresh_interval: self.refresh_interval(),
            wait,
        }
    }

    pub fn presentation(&self) -> ProgressPresentation {
        if self.quiet {
            ProgressPresentation::Hidden
        } else if self.plain {
            ProgressPresentation::Plain
        } else {
            ProgressPresentation::Progress
        }
    }

    pub fn jobs(&self) -> usize {
        self.jobs
            .map(usize::from)
            .unwrap_or(usize::MAX)
    }

    pub fn refresh_interval(&self) -> Duration {
        self.refresh
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
    inspect,
    health
]);
