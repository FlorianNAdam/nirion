use clap::Parser;
use std::collections::BTreeMap;

use crate::{compose_target_cmd, Project, TargetSelector};

#[derive(Parser, Debug, Clone)]
pub struct UpArgs {
    /// Target selector: *, project, or project.service
    #[arg(default_value = "*", value_parser = crate::clap_parse_selector)]
    pub target: TargetSelector,
}

pub fn handle_up(
    args: &UpArgs,
    projects: &BTreeMap<String, Project>,
) -> anyhow::Result<()> {
    compose_target_cmd(&args.target, projects, &["up", "-d"])
}
