use clap::Parser;
use std::{collections::BTreeMap, path::Path};

use crate::{
    docker::compose_target_cmd, ClapSelector, Project, TargetSelector,
};

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
    projects: &BTreeMap<String, Project>,
    _locked_images: &BTreeMap<String, String>,
    _lock_file: &Path,
) -> anyhow::Result<()> {
    // docker compose top has no flags: just ["top"]
    let cmd: Vec<&str> = vec!["top"];

    compose_target_cmd(&args.target, projects, &cmd).await
}
