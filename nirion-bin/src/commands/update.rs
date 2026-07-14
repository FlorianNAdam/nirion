use clap::Parser;
use futures::StreamExt;
use nirion_lib::{
    context::NirionContext,
    lock_update::update_images,
    projects::{get_images, TargetSelector},
};

use crate::{commands::lock::render_lock_update_event, ClapSelector};

/// Update lock file entries
#[derive(Parser, Debug, Clone)]
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
    context: &NirionContext,
) -> anyhow::Result<()> {
    let images = get_images(&args.target, &context.projects);
    let mut operation = update_images(context, images, args.jobs);

    while let Some(event) = operation.events.next().await {
        render_lock_update_event(event?);
    }

    operation.finish().await?;

    Ok(())
}
