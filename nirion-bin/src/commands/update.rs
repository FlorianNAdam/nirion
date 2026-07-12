use std::path::Path;

use clap::Parser;
use futures::StreamExt;
use nirion_lib::{
    lock::LockedImages,
    lock_update::update_images,
    projects::{get_images, Projects, TargetSelector},
};
use nirion_oci_lib::client::AuthConfig;

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
    projects: &Projects,
    locked_images: &LockedImages,
    lock_file: &Path,
    auth: &AuthConfig,
) -> anyhow::Result<()> {
    let images = get_images(&args.target, projects);
    let mut operation = update_images(
        auth.clone(),
        images,
        locked_images.clone(),
        lock_file.to_path_buf(),
        args.jobs,
    );

    while let Some(event) = operation.events.next().await {
        render_lock_update_event(event?);
    }

    operation.finish().await?;

    Ok(())
}
