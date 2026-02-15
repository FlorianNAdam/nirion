use std::path::Path;

use clap::Parser;
use nirion_lib::{
    auth::AuthConfig,
    lock::LockedImages,
    projects::{get_images, Projects, TargetSelector},
};

use crate::{lock::update_images, ClapSelector};

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
    update_images(auth, images, locked_images, lock_file, args.jobs).await
}
