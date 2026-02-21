use crossterm::{cursor, execute, style::Stylize};
use futures::{stream::FuturesUnordered, StreamExt};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use nirion_lib::{
    auth::AuthConfig,
    lock::{LockedImages, VersionedImage},
};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::stdout,
    path::Path,
    sync::Arc,
    time::Duration,
};
use tokio::sync::RwLock;

use nirion_oci_lib::{
    get_updated_versioned_image, get_versioned_image,
    oci_client::{Client, Reference},
};

pub async fn update_images(
    auth: &AuthConfig,
    images: BTreeMap<String, String>,
    locked_images: &LockedImages,
    lock_file: &Path,
    jobs: usize,
) -> anyhow::Result<()> {
    let total_images = images.len();

    if total_images == 0 {
        println!("No images found to update");
        return Ok(());
    }

    let mut stdout = stdout();
    execute!(stdout, cursor::Hide)?;

    let multi_progress = MultiProgress::new();
    let overall_pb = multi_progress.add(ProgressBar::new(total_images as u64));
    overall_pb.set_style(
        ProgressStyle::default_bar()
            .template(
                "[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} {msg}",
            )
            .unwrap()
            .progress_chars("██"),
    );

    overall_pb.enable_steady_tick(Duration::from_millis(100));

    overall_pb.set_message("Starting...");

    let digest_cache: Arc<RwLock<HashMap<String, VersionedImage>>> =
        Arc::new(RwLock::new(HashMap::new()));
    let client = Arc::new(auth.get_oci_client().await);

    let semaphore = Arc::new(tokio::sync::Semaphore::new(jobs));

    let mut futures = FuturesUnordered::new();

    for (service, image) in images {
        let client = Arc::clone(&client);
        let semaphore = Arc::clone(&semaphore);
        let digest_cache = Arc::clone(&digest_cache);
        let overall_pb = overall_pb.clone();
        let multi_progress = multi_progress.clone();

        let current_versioned_image = locked_images.get(&service).cloned();

        futures.push(async move {
            let _permit = semaphore.acquire().await.unwrap();

            let pb = multi_progress.add(ProgressBar::new_spinner());
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template("{spinner:.green} {msg}")
                    .unwrap()
                    .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
            );
            pb.enable_steady_tick(Duration::from_millis(100));
            pb.set_message(format!("Checking {}", image));

            let versioned_image = if let Some(current) = current_versioned_image
            {
                get_cached_updated_image(&client, &current, &digest_cache)
                    .await?
            } else {
                get_cached_image(&client, &image, &digest_cache).await?
            };

            pb.finish_and_clear();

            overall_pb.inc(1);

            Ok::<_, anyhow::Error>((service, versioned_image))
        });
    }

    let mut new_locked_images = locked_images.clone();

    while let Some(result) = futures.next().await {
        let (service, versioned_image) = result?;
        new_locked_images.insert(service, versioned_image);
    }

    overall_pb.finish_with_message("All images checked");
    println!();

    if locked_images == &new_locked_images {
        println!("All images are already up-to-date");
        return Ok(());
    }

    println!("\nChanges:");
    print_diff(&locked_images, &new_locked_images);

    println!("\nUpdating lock file...");
    let new_lock_file = serde_json::to_string_pretty(&new_locked_images)?;
    fs::write(lock_file, new_lock_file)?;

    println!("Lock file updated successfully");

    Ok(())
}

fn print_diff(old: &LockedImages, new: &LockedImages) {
    for entry in old.diff(new) {
        use nirion_lib::lock::DiffEntry::*;
        match entry {
            Added { service, new } => {
                println!("  + {}:", service.to_string().green());
                if let Some(version) = &new.version {
                    println!("      new version: {}", version);
                }
                println!("      new digest: {}", new.digest);
            }
            Updated { service, old, new } => {
                println!("  ~ {}:", service.to_string().cyan());
                if let Some(version) = &new.version {
                    let old_version = old
                        .version
                        .as_ref()
                        .map(|s| s.as_str())
                        .unwrap_or("none");

                    println!(
                        "      new version: {} -> {}",
                        old_version, version
                    );
                }
                println!("      old digest: {}", old.digest);
                println!("      new digest: {}", new.digest);
            }
            Removed { service, old } => {
                println!("  - {}:", service.to_string().yellow());
                if let Some(version) = &old.version {
                    println!("      old version: {}", version);
                }
                println!("      old digest: {}", old.digest);
            }
        }
    }
}

async fn get_cached_image(
    client: &Client,
    image: &str,
    cache: &Arc<RwLock<HashMap<String, VersionedImage>>>,
) -> anyhow::Result<VersionedImage> {
    if let Some(existing) = {
        let locked_cache = cache.read().await;
        locked_cache.get(image).cloned()
    } {
        return Ok(existing);
    }

    let reference = Reference::try_from(image)?;
    let versioned_image = get_versioned_image(&client, &reference).await?;

    {
        let mut locked_cache = cache.write().await;
        locked_cache.insert(image.to_string(), versioned_image.clone());
    }

    Ok(versioned_image)
}

async fn get_cached_updated_image(
    client: &Client,
    versioned_image: &VersionedImage,
    cache: &Arc<RwLock<HashMap<String, VersionedImage>>>,
) -> anyhow::Result<VersionedImage> {
    let image = versioned_image.image.as_str();

    if let Some(existing) = {
        let locked_cache = cache.read().await;
        locked_cache.get(image).cloned()
    } {
        return Ok(existing);
    }

    let versioned_image =
        get_updated_versioned_image(&client, &versioned_image).await?;

    {
        let mut locked_cache = cache.write().await;
        locked_cache.insert(image.to_string(), versioned_image.clone());
    }

    Ok(versioned_image)
}
