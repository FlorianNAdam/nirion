use clap::Args;
use futures::StreamExt;
use nirion_lib::{
    backend::{LockUpdateMode, LockUpdateOperation, NirionBackend},
    projects::TargetSelector,
};

use crate::{commands::lock::format_lock_update_event, ClapSelector};

/// Update lock file entries
#[derive(Args, Debug, Clone)]
pub struct UpdateArgs {
    /// Target selector: *, project, or project.service
    #[arg(
        default_value = "*",
        value_parser = TargetSelector::clap_parse,
        add = TargetSelector::clap_completer()
    )]
    pub target: TargetSelector,

    /// Number of concurrent digest fetches
    #[arg(short = 'j', long = "jobs", default_value_t = 10)]
    pub jobs: usize,
}

pub async fn handle_update(
    args: &UpdateArgs,
    backend: &dyn NirionBackend,
) -> anyhow::Result<()> {
    let mut events = backend
        .lock_updates(LockUpdateOperation {
            target: args.target.clone(),
            mode: LockUpdateMode::UpdateAll,
            jobs: args.jobs,
        })
        .await;

    while let Some(event) = events.next().await {
        println!("{}", format_lock_update_event(event?));
    }

    Ok(())
}
