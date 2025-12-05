use std::collections::BTreeMap;

use crate::{compose_target_cmd, Project, TargetSelector};

pub fn handle_up(
    target: &TargetSelector,
    projects: &BTreeMap<String, Project>,
) -> anyhow::Result<()> {
    compose_target_cmd(target, projects, &["up", "-d"])
}
