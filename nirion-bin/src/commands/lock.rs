use std::{collections::BTreeMap, path::Path};

use clap::Parser;
use nirion_lib::projects::{get_images, Projects, TargetSelector};

use crate::{lock::update_images, ClapSelector};

/// Create missing lock file entries
#[derive(Parser, Debug, Clone)]
pub struct LockArgs {
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

pub async fn handle_lock(
    args: &LockArgs,
    projects: &Projects,
    locked_images: &BTreeMap<String, String>,
    lock_file: &Path,
) -> anyhow::Result<()> {
    let mut images = get_images(&args.target, projects);
    images.retain(|name, _| !locked_images.contains_key(name));

    update_images(images, locked_images, lock_file, args.jobs).await
}
