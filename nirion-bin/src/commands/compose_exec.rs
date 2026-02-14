use clap::Parser;
use nirion_lib::projects::{Project, TargetSelector};
use std::{collections::BTreeMap, path::Path};

use crate::{docker::compose_target_cmd, ClapSelector};

/// Run a docker-compose command for a project or service
#[derive(Parser, Debug, Clone)]
pub struct ComposeExecArgs {
    /// Target selector: *, project, or project.service
    #[arg(
        default_value = "*",
        value_parser = TargetSelector::clap_parse,
        add = TargetSelector::clap_completer()
    )]
    pub target: TargetSelector,

    /// Command to execute in container
    cmd: Vec<String>,
}

pub async fn handle_compose_exec(
    args: &ComposeExecArgs,
    projects: &BTreeMap<String, Project>,
    _locked_images: &BTreeMap<String, String>,
    _lock_file: &Path,
) -> anyhow::Result<()> {
    let cmd_slices: Vec<&str> = args
        .cmd
        .iter()
        .map(|s| s.as_str())
        .collect();

    compose_target_cmd(&args.target, projects, &cmd_slices).await
}
