use std::{collections::BTreeMap, path::Path};

use clap::Parser;

use crate::{get_images, lock::update_images, Project, TargetSelector};

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
    projects: &BTreeMap<String, Project>,
    locked_images: &BTreeMap<String, String>,
    lock_file: &Path,
) -> anyhow::Result<()> {
    let images = get_images(&args.target, projects);
    update_images(images, locked_images, lock_file, args.jobs).await
}
