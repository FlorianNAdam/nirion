use anyhow::Result;
use std::collections::BTreeMap;

use crate::{compose_target_cmd, Project, TargetSelector};

pub fn handle_down(
    target: &TargetSelector,
    projects: &BTreeMap<String, Project>,
) -> Result<()> {
    compose_target_cmd(target, projects, &["down"])
}
