use std::collections::BTreeMap;

use crate::{fetch_digest, get_images, Project, TargetSelector};

pub async fn handle_update(
    target: &TargetSelector,
    projects: &BTreeMap<String, Project>,
) -> anyhow::Result<()> {
    let images = get_images(target, projects);

    for (_service, image) in images {
        let digest = fetch_digest(&image)?;
        println!("{}", digest);
    }

    Ok(())
}
