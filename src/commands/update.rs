use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::Path,
};

use crate::{fetch_digest, get_images, Project, TargetSelector};

pub async fn handle_update(
    target: &TargetSelector,
    projects: &BTreeMap<String, Project>,
    locked_images: &BTreeMap<String, String>,
    lock_file: &Path,
) -> anyhow::Result<()> {
    let images = get_images(target, projects);

    let mut digest_cache: HashMap<String, String> = HashMap::new();
    let mut new_digests = BTreeMap::new();

    for (service, image) in images {
        println!("Checking {} ({})", service, image);
        let digest = if let Some(digest) = digest_cache.get(&image) {
            digest.to_string()
        } else {
            let digest = fetch_digest(&image)?;
            digest_cache.insert(image, digest.clone());
            digest
        };

        if let Some(old_digest) = locked_images.get(&service) {
            if old_digest != &digest {
                println!("Digest changed: {} -> {}", old_digest, digest);
                new_digests.insert(service, digest);
            } else {
                println!("Already up-to-date")
            }
        } else {
            println!("New digest: {}", digest);
            new_digests.insert(service, digest);
        }
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
