use paste::paste;

use clap::{Args, Subcommand};
use nirion_lib::backend::NirionBackend;
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
                backend: &dyn NirionBackend
            ) -> anyhow::Result<()> {
                match command {
                    $(
                        Commands::[<$modname:camel>] { args } =>
                            [<handle_ $modname>](args, backend).await?,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroUsize;

    fn lifecycle_args(
        plain: bool,
        quiet: bool,
        jobs: Option<usize>,
    ) -> LifecycleArgs {
        LifecycleArgs {
            plain,
            refresh: Duration::from_millis(123),
            quiet,
            jobs: jobs.map(|jobs| NonZeroUsize::new(jobs).unwrap()),
        }
    }

    #[test]
    fn lifecycle_presentation_defaults_to_progress() {
        assert_eq!(
            lifecycle_args(false, false, None).presentation(),
            ProgressPresentation::Progress
        );
    }

    #[test]
    fn lifecycle_presentation_prefers_quiet_over_plain() {
        assert_eq!(
            lifecycle_args(true, true, None).presentation(),
            ProgressPresentation::Hidden
        );
        assert_eq!(
            lifecycle_args(true, false, None).presentation(),
            ProgressPresentation::Plain
        );
    }

    #[test]
    fn lifecycle_jobs_defaults_to_unbounded_or_uses_configured_value() {
        assert_eq!(lifecycle_args(false, false, None).jobs(), usize::MAX);
        assert_eq!(lifecycle_args(false, false, Some(3)).jobs(), 3);
    }

    #[test]
    fn lifecycle_options_include_refresh_jobs_presentation_and_wait_target() {
        let options = lifecycle_args(true, false, Some(2))
            .options(WaitTarget::Healthchecks);

        assert_eq!(options.presentation, ProgressPresentation::Plain);
        assert_eq!(options.jobs, 2);
        assert_eq!(options.refresh_interval, Duration::from_millis(123));
        assert_eq!(options.wait, WaitTarget::Healthchecks);
    }
}
