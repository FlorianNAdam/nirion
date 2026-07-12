use futures::{FutureExt, stream::FuturesUnordered};
use futures::{StreamExt, channel::mpsc, stream::BoxStream};
use nirion_oci_lib::{
    client::{AuthConfig, NirionOciClient},
    oci_client::Reference,
};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::PathBuf,
    sync::Arc,
};
use tokio::{sync::RwLock, task::JoinHandle};

use crate::{
    events::LockUpdateEvent,
    lock::{DiffEntry, LockedImages, VersionedImage},
};

#[derive(Clone)]
pub struct LockUpdateReport {
    pub locked_images: LockedImages,
    pub diffs: Vec<DiffEntry>,
    pub written: bool,
}

pub struct LockUpdateOperation {
    pub events: BoxStream<'static, anyhow::Result<LockUpdateEvent>>,
    report: JoinHandle<anyhow::Result<LockUpdateReport>>,
}

impl LockUpdateOperation {
    pub async fn finish(self) -> anyhow::Result<LockUpdateReport> {
        self.report.await?
    }
}

pub fn update_images(
    auth: AuthConfig,
    images: BTreeMap<String, String>,
    locked_images: LockedImages,
    lock_file: PathBuf,
    jobs: usize,
) -> LockUpdateOperation {
    let (event_tx, event_rx) = mpsc::unbounded();

    let report = tokio::spawn(async move {
        if images.is_empty() {
            let _ = event_tx.unbounded_send(Ok(LockUpdateEvent::NoImages));
            return Ok(LockUpdateReport {
                locked_images,
                diffs: Vec::new(),
                written: false,
            });
        }

        let digest_cache: Arc<RwLock<HashMap<String, VersionedImage>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let client = Arc::new(
            NirionOciClient::builder()
                .auth(auth)
                .build(),
        );
        let semaphore = Arc::new(tokio::sync::Semaphore::new(jobs));
        let mut futures = FuturesUnordered::new();

        for (service, image) in images {
            let _ =
                event_tx.unbounded_send(Ok(LockUpdateEvent::ImageStarted {
                    service: service.clone(),
                    image: image.clone(),
                }));

            let client = Arc::clone(&client);
            let semaphore = Arc::clone(&semaphore);
            let digest_cache = Arc::clone(&digest_cache);
            let current_versioned_image = locked_images.get(&service).cloned();
            let event_tx = event_tx.clone();

            futures.push(
                async move {
                    let _permit = semaphore.acquire().await.unwrap();

                    let versioned_image = if let Some(mut current) =
                        current_versioned_image
                    {
                        let reference = Reference::try_from(image)?;
                        current.image = reference.to_string();
                        get_cached_updated_image(
                            &client,
                            &current,
                            &digest_cache,
                        )
                        .await?
                    } else {
                        get_cached_image(&client, &image, &digest_cache).await?
                    };

                    let _ = event_tx.unbounded_send(Ok(
                        LockUpdateEvent::ImageResolved {
                            service: service.clone(),
                        },
                    ));

                    Ok::<_, anyhow::Error>((service, versioned_image))
                }
                .boxed(),
            );
        }

        let mut new_locked_images = locked_images.clone();

        while let Some(result) = futures.next().await {
            let (service, versioned_image) = result?;
            new_locked_images.insert(service, versioned_image);
        }

        let diffs = locked_images.diff(&new_locked_images);

        if diffs.is_empty() {
            let _ = event_tx.unbounded_send(Ok(LockUpdateEvent::UpToDate));
            return Ok(LockUpdateReport {
                locked_images: new_locked_images,
                diffs,
                written: false,
            });
        }

        let _ = event_tx.unbounded_send(Ok(LockUpdateEvent::ChangesDetected {
            diffs: diffs.clone(),
        }));
        let _ = event_tx.unbounded_send(Ok(LockUpdateEvent::WritingLockFile));

        let new_lock_file = serde_json::to_string_pretty(&new_locked_images)?;
        fs::write(lock_file, new_lock_file)?;

        let _ = event_tx.unbounded_send(Ok(LockUpdateEvent::LockFileWritten));

        Ok(LockUpdateReport {
            locked_images: new_locked_images,
            diffs,
            written: true,
        })
    });

    LockUpdateOperation {
        events: event_rx.boxed(),
        report,
    }
}

async fn get_cached_image(
    client: &NirionOciClient,
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
    let versioned_image = client
        .get_versioned_image(&reference)
        .await?;

    {
        let mut locked_cache = cache.write().await;
        locked_cache.insert(image.to_string(), versioned_image.clone());
    }

    Ok(versioned_image)
}

async fn get_cached_updated_image(
    client: &NirionOciClient,
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

    let versioned_image = client
        .get_updated_versioned_image(versioned_image)
        .await?;

    {
        let mut locked_cache = cache.write().await;
        locked_cache.insert(image.to_string(), versioned_image.clone());
    }

    Ok(versioned_image)
}
