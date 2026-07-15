use futures::{FutureExt, stream::FuturesUnordered};
use futures::{StreamExt, channel::mpsc, stream::BoxStream};
use nirion_oci_lib::{client::NirionOciClient, oci_client::Reference};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    sync::Arc,
};
use tokio::{sync::RwLock, task::JoinHandle};

use crate::{
    context::NirionContext,
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
    context: &NirionContext,
    images: BTreeMap<String, String>,
    jobs: usize,
) -> LockUpdateOperation {
    let client = context.oci_client.clone();
    let locked_images = context.locked_images.clone();
    let lock_file = context.lock_file.clone();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        docker::DockerCommand, events::LockUpdateEvent, projects::Projects,
    };
    use futures::StreamExt;
    use nirion_oci_lib::{
        oci_client::secrets::RegistryAuth,
        test_registry::{RegistryHandle, http_nirion_client},
    };
    use std::path::PathBuf;

    fn image(
        image: &str,
        version: &str,
        digest: &str,
    ) -> VersionedImage {
        VersionedImage {
            image: image.to_string(),
            version: Some(version.to_string()),
            digest: digest.to_string(),
        }
    }

    fn context(
        client: NirionOciClient,
        locked_images: LockedImages,
        lock_file: PathBuf,
    ) -> NirionContext {
        NirionContext {
            projects: Projects::default(),
            locked_images,
            lock_file,
            oci_client: Arc::new(client),
            docker_command: DockerCommand::default(),
        }
    }

    #[tokio::test]
    async fn no_images_reports_no_images_without_writing() -> anyhow::Result<()>
    {
        let dir = tempfile::tempdir()?;
        let lock_file = dir.path().join("nirion.lock");
        let mut operation = update_images(
            &context(
                NirionOciClient::builder().build(),
                LockedImages::default(),
                lock_file.clone(),
            ),
            BTreeMap::new(),
            1,
        );

        assert!(matches!(
            operation
                .events
                .next()
                .await
                .transpose()?,
            Some(LockUpdateEvent::NoImages)
        ));

        let report = operation.finish().await?;
        assert!(!report.written);
        assert!(report.diffs.is_empty());
        assert!(!lock_file.exists());

        Ok(())
    }

    #[tokio::test]
    async fn adds_new_image_and_writes_lock_file() -> anyhow::Result<()> {
        let handle = RegistryHandle::start_anonymous().await?;
        let test_image = handle
            .push(
                "library/nirion-lock-update",
                "1.2.3",
                &RegistryAuth::Anonymous,
            )
            .await?;
        let dir = tempfile::tempdir()?;
        let lock_file = dir.path().join("nirion.lock");
        let report = update_images(
            &context(
                http_nirion_client().build(),
                LockedImages::default(),
                lock_file.clone(),
            ),
            BTreeMap::from([(
                "app.web".to_string(),
                test_image.reference.to_string(),
            )]),
            1,
        )
        .finish()
        .await?;

        assert!(report.written);
        assert!(
            matches!(report.diffs.as_slice(), [DiffEntry::Added { service, new }] if service == "app.web" && new.digest == test_image.digest)
        );
        assert_eq!(
            report
                .locked_images
                .get("app.web")
                .unwrap()
                .digest,
            test_image.digest
        );

        let written: LockedImages =
            serde_json::from_str(&std::fs::read_to_string(lock_file)?)?;
        assert_eq!(written.get("app.web").unwrap().digest, test_image.digest);

        Ok(())
    }

    #[tokio::test]
    async fn unchanged_locked_image_reports_up_to_date() -> anyhow::Result<()> {
        let handle = RegistryHandle::start_anonymous().await?;
        let test_image = handle
            .push(
                "library/nirion-lock-update",
                "1.2.3",
                &RegistryAuth::Anonymous,
            )
            .await?;
        let dir = tempfile::tempdir()?;
        let lock_file = dir.path().join("nirion.lock");
        let mut locked_images = LockedImages::default();
        locked_images.insert(
            "app.web".to_string(),
            image(
                &test_image.reference.to_string(),
                "1.2.3",
                &test_image.digest,
            ),
        );

        let report = update_images(
            &context(
                http_nirion_client().build(),
                locked_images,
                lock_file.clone(),
            ),
            BTreeMap::from([(
                "app.web".to_string(),
                test_image.reference.to_string(),
            )]),
            1,
        )
        .finish()
        .await?;

        assert!(!report.written);
        assert!(report.diffs.is_empty());
        assert!(!lock_file.exists());

        Ok(())
    }

    #[tokio::test]
    async fn stale_locked_image_updates_digest_and_writes_lock_file()
    -> anyhow::Result<()> {
        let handle = RegistryHandle::start_anonymous().await?;
        let test_image = handle
            .push(
                "library/nirion-lock-update",
                "1.2.3",
                &RegistryAuth::Anonymous,
            )
            .await?;
        let dir = tempfile::tempdir()?;
        let lock_file = dir.path().join("nirion.lock");
        let mut locked_images = LockedImages::default();
        locked_images.insert(
            "app.web".to_string(),
            image(
                &test_image.reference.to_string(),
                "1.0.0",
                "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            ),
        );

        let report = update_images(
            &context(http_nirion_client().build(), locked_images, lock_file),
            BTreeMap::from([(
                "app.web".to_string(),
                test_image.reference.to_string(),
            )]),
            1,
        )
        .finish()
        .await?;

        assert!(report.written);
        assert!(
            matches!(report.diffs.as_slice(), [DiffEntry::Updated { service, new, .. }] if service == "app.web" && new.digest == test_image.digest)
        );
        assert_eq!(
            report
                .locked_images
                .get("app.web")
                .unwrap()
                .digest,
            test_image.digest
        );

        Ok(())
    }

    #[tokio::test]
    async fn invalid_image_reference_returns_error() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let lock_file = dir.path().join("nirion.lock");
        let result = update_images(
            &context(
                http_nirion_client().build(),
                LockedImages::default(),
                lock_file.clone(),
            ),
            BTreeMap::from([(
                "app.web".to_string(),
                "not a valid image".to_string(),
            )]),
            1,
        )
        .finish()
        .await;

        let err = match result {
            Ok(_) => panic!("expected invalid image reference to fail"),
            Err(err) => err,
        };

        assert!(!err.to_string().is_empty());
        assert!(!lock_file.exists());

        Ok(())
    }
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
