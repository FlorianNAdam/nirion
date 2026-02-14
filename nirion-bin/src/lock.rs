use crossterm::{cursor, execute, style::Stylize};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::stdout,
    path::Path,
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;

use nirion_oci_lib::oci_client::Client;
use nirion_oci_lib::{get_version_and_digest, oci_client::Reference};

pub async fn update_images(
    images: BTreeMap<String, String>,
    locked_images: &BTreeMap<String, String>,
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

    let digest_cache: Arc<Mutex<HashMap<String, (String, Option<String>)>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let new_digests: Arc<Mutex<BTreeMap<String, (String, Option<String>)>>> =
        Arc::new(Mutex::new(BTreeMap::new()));
    let locked_images = Arc::new(locked_images.clone());

    let semaphore = Arc::new(tokio::sync::Semaphore::new(jobs));
    let mut tasks = Vec::new();

    for (service, image) in images {
        let semaphore = Arc::clone(&semaphore);
        let digest_cache = Arc::clone(&digest_cache);
        let new_digests = Arc::clone(&new_digests);
        let locked_images = Arc::clone(&locked_images);
        let overall_pb = overall_pb.clone();
        let multi_progress = multi_progress.clone();

        let task = tokio::spawn(async move {
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

            let result = process_image(
                &service,
                &image,
                &digest_cache,
                &locked_images,
                &new_digests,
                pb,
            )
            .await;

            overall_pb.inc(1);
            overall_pb.set_message(format!(
                "Processed {}/{}",
                overall_pb.position(),
                total_images
            ));

            result
        });

        tasks.push(task);
    }

    let mut results = Vec::new();
    for task in tasks {
        results.push(task.await?);
    }
    for result in results {
        result?;
    }

    overall_pb.finish_with_message("All images checked");
    println!();

    let new_digests = Arc::try_unwrap(new_digests)
        .unwrap_or_else(|_| panic!("Failed to unwrap new_digests"))
        .into_inner();

    if new_digests.is_empty() {
        println!("All images are already up-to-date");
        return Ok(());
    }

    println!("\nDigest changes:");
    for (service, new_digest) in &new_digests {
        // let version_string = if let Some(version) = new_digest.1.as_ref() {
        //     format!(" ({})", version)
        // } else {
        //     String::new()
        // };

        match locked_images.get(service) {
            Some(old_digest) => {
                println!(
                    "  ~ {}:\n      old: {}\n      new: {}",
                    service.to_string().cyan(),
                    old_digest,
                    new_digest.0
                );
            }
            None => {
                println!(
                    "  + {}:\n      new: {}",
                    service.to_string().green(),
                    new_digest.0
                );
            }
        }
    }

    let new_digests = new_digests
        .into_iter()
        .map(|(k, v)| (k, v.0))
        .collect::<BTreeMap<String, String>>();

    println!("\nUpdating lock file...");
    let mut new_locked_images = locked_images.as_ref().clone();
    new_locked_images.extend(new_digests);
    let new_lock_file = serde_json::to_string_pretty(&new_locked_images)?;
    fs::write(lock_file, new_lock_file)?;

    println!("Lock file updated successfully");

    Ok(())
}

async fn process_image(
    service: &str,
    image: &str,
    cache: &Arc<Mutex<HashMap<String, (String, Option<String>)>>>,
    locked_images: &Arc<BTreeMap<String, String>>,
    new_digests: &Arc<Mutex<BTreeMap<String, (String, Option<String>)>>>,
    pb: ProgressBar,
) -> anyhow::Result<()> {
    let digest = {
        let locked_cache = cache.lock().await;
        if let Some(digest) = locked_cache.get(image) {
            pb.set_message(format!("Cache hit for {}", image));
            digest.clone()
        } else {
            drop(locked_cache); // Release lock before async operation

            pb.set_message(format!("Fetching digest for {}", image));

            let reference = Reference::try_from(image)?;

            let client = Client::default();
            let (version, digest) =
                get_version_and_digest(&client, &reference).await?;

            let result = (digest, version);

            let mut cache = cache.lock().await;
            cache.insert(image.to_string(), result.clone());
            result
        }
    };

    let mut new_digests = new_digests.lock().await;

    if let Some(old_digest) = locked_images.get(service) {
        if old_digest != &digest.0 {
            pb.set_message(format!("✓ {}: Updated", service));
            new_digests.insert(service.to_string(), digest);
        } else {
            pb.set_message(format!("✓ {}: Up-to-date", service));
        }
    } else {
        pb.set_message(format!("✓ {}: New image", service));
        new_digests.insert(service.to_string(), digest);
    }

    pb.finish_and_clear();
    Ok(())
}
