use clap::Parser;
use std::{collections::BTreeMap, path::Path};

use crate::{
    clap_parse_selector, docker::compose_target_cmd, Project, TargetSelector,
};

#[derive(Parser, Debug, Clone)]
pub struct ComposeExecArgs {
    #[arg(value_parser = clap_parse_selector)]
    target: TargetSelector,

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
