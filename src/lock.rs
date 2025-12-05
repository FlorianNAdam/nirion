use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::Path,
};

use crate::{fetch_digest, get_images, Project, TargetSelector};

pub async fn handle_lock(
    target: &TargetSelector,
    projects: &BTreeMap<String, Project>,
    locked_images: &BTreeMap<String, String>,
    lock_file: &Path,
) -> anyhow::Result<()> {
    let images = get_images(target, projects);

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
            let digest = fetch_digest(&image)?;
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
