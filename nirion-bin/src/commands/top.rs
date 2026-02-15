use clap::Parser;
use nirion_lib::{auth::AuthConfig, lock::LockedImages, projects::Projects};
use std::path::Path;

use crate::{docker::compose_target_cmd, ClapSelector, TargetSelector};

/// Display the running processes of a service container
#[derive(Parser, Debug, Clone)]
pub struct TopArgs {
    /// Target selector: *, project, or project.service
    #[arg(
        default_value = "*",
        value_parser = TargetSelector::clap_parse,
        add = TargetSelector::clap_completer()
    )]
    pub target: TargetSelector,
}

pub async fn handle_top(
    args: &TopArgs,
    projects: &Projects,
    _locked_images: &LockedImages,
    _lock_file: &Path,
    _auth: &AuthConfig,
) -> anyhow::Result<()> {
    // docker compose top has no flags: just ["top"]
    let cmd: Vec<&str> = vec!["top"];

    compose_target_cmd(&args.target, projects, &cmd).await
}
