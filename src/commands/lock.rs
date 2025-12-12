use clap::Parser;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::Path,
};

use crate::{docker::fetch_digest, get_images, Project, TargetSelector};

#[derive(Parser, Debug, Clone)]
pub struct LockArgs {
    /// Target selector: *, project, or project.service
    #[arg(default_value = "*", value_parser = crate::clap_parse_selector)]
    pub target: TargetSelector,
}

pub async fn handle_lock(
    args: &LockArgs,
    projects: &BTreeMap<String, Project>,
    locked_images: &BTreeMap<String, String>,
    lock_file: &Path,
) -> anyhow::Result<()> {
    let images = get_images(&args.target, projects);

    let mut digest_cache: HashMap<String, String> = HashMap::new();
    let mut new_digests = BTreeMap::new();

    for (service, image) in images {
        if locked_images.contains_key(&service) {
            continue;
        }

        println!("Checking {} ({})", service, image);
        let digest = if let Some(digest) = digest_cache.get(&image) {
            digest.to_string()
        } else {
            let digest = fetch_digest(&image).await?;
            digest_cache.insert(image, digest.clone());
            digest
        };

        println!("New digest: {}", digest);
        new_digests.insert(service, digest);
    }

    if new_digests.is_empty() {
        return Ok(());
    }

    let mut new_locked_images = locked_images.clone();
    new_locked_images.extend(new_digests);
    let new_lock_file = serde_json::to_string_pretty(&new_locked_images)?;
    fs::write(lock_file, new_lock_file)?;

    Ok(())
}
