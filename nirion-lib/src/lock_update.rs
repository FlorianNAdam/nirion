use futures::{FutureExt, stream::FuturesUnordered};
use futures::{StreamExt, channel::mpsc, stream::BoxStream};
use nirion_oci_lib::{
    client::{NirionOciClient, VersionedImageResolution},
    oci_client::Reference,
};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    sync::Arc,
};
use tokio::sync::RwLock;

use crate::{
    context::NirionContext,
    events::LockUpdateEvent,
    lock::{LockedImages, VersionedImage},
};

pub fn image_update_stream(
    context: &NirionContext,
    images: BTreeMap<String, String>,
    jobs: usize,
) -> BoxStream<'static, anyhow::Result<LockUpdateEvent>> {
    let client = context.oci_client.clone();
    let locked_images = context.locked_images.clone();
    let lock_file = context.lock_file.clone();
    let (event_tx, event_rx) = mpsc::unbounded();

    tokio::spawn(async move {
        if let Err(error) = image_update_stream_inner(
            client,
            locked_images,
            lock_file,
            images,
            jobs,
            Some(event_tx.clone()),
        )
        .await
        {
            let _ = event_tx.unbounded_send(Err(error));
        }
    });

    event_rx.boxed()
}

async fn image_update_stream_inner(
    client: Arc<NirionOciClient>,
    locked_images: LockedImages,
    lock_file: std::path::PathBuf,
    images: BTreeMap<String, String>,
    jobs: usize,
    event_tx: Option<mpsc::UnboundedSender<anyhow::Result<LockUpdateEvent>>>,
) -> anyhow::Result<()> {
    if images.is_empty() {
        emit_event(&event_tx, LockUpdateEvent::NoImages);
        return Ok(());
    }

    let digest_cache: Arc<RwLock<HashMap<String, VersionedImage>>> =
        Arc::new(RwLock::new(HashMap::new()));
    let semaphore = Arc::new(tokio::sync::Semaphore::new(jobs.max(1)));
    let mut futures = FuturesUnordered::new();

    for (service, image) in images {
        emit_event(
            &event_tx,
            LockUpdateEvent::ImageStarted {
                service: service.clone(),
                image: image.clone(),
            },
        );

        let client = Arc::clone(&client);
        let semaphore = Arc::clone(&semaphore);
        let digest_cache = Arc::clone(&digest_cache);
        let current_versioned_image = locked_images.get(&service).cloned();
        let event_tx = event_tx.clone();

        futures.push(
            async move {
                let _permit = semaphore.acquire().await.unwrap();

                let resolved = if let Some(mut current) =
                    current_versioned_image
                {
                    current.image = image.clone();
                    get_cached_updated_image(&client, &current, &digest_cache)
                        .await?
                } else {
                    get_cached_image(&client, &image, &digest_cache).await?
                };

                for warning in resolved.warnings {
                    emit_event(
                        &event_tx,
                        LockUpdateEvent::Warning {
                            service: service.clone(),
                            message: warning,
                        },
                    );
                }

                emit_event(
                    &event_tx,
                    LockUpdateEvent::ImageResolved {
                        service: service.clone(),
                    },
                );

                Ok::<_, anyhow::Error>((service, resolved.image))
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
        emit_event(&event_tx, LockUpdateEvent::UpToDate);
        return Ok(());
    }

    emit_event(
        &event_tx,
        LockUpdateEvent::ChangesDetected {
            diffs: diffs.clone(),
        },
    );
    emit_event(&event_tx, LockUpdateEvent::WritingLockFile);

    let new_lock_file = serde_json::to_string_pretty(&new_locked_images)?;
    fs::write(lock_file, new_lock_file)?;

    emit_event(&event_tx, LockUpdateEvent::LockFileWritten);

    Ok(())
}

fn emit_event(
    tx: &Option<mpsc::UnboundedSender<anyhow::Result<LockUpdateEvent>>>,
    event: LockUpdateEvent,
) {
    if let Some(tx) = tx {
        let _ = tx.unbounded_send(Ok(event));
    }
}

async fn get_cached_image(
    client: &NirionOciClient,
    image: &str,
    cache: &Arc<RwLock<HashMap<String, VersionedImage>>>,
) -> anyhow::Result<VersionedImageResolution> {
    if let Some(existing) = {
        let locked_cache = cache.read().await;
        locked_cache.get(image).cloned()
    } {
        return Ok(VersionedImageResolution {
            image: existing,
            warnings: Vec::new(),
        });
    }

    let reference = Reference::try_from(image)?;
    let mut resolved = client
        .get_versioned_image_resolution(&reference)
        .await?;
    resolved.image.image = image.to_string();

    {
        let mut locked_cache = cache.write().await;
        locked_cache.insert(image.to_string(), resolved.image.clone());
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        docker::DockerCommand, events::LockUpdateEvent, lock::DiffEntry,
        projects::Projects,
    };
    use futures::StreamExt;
    use nirion_oci_lib::{
        docker_hub::DockerHubClient,
        oci_client::secrets::RegistryAuth,
        test_registry::{RegistryHandle, http_nirion_client},
    };
    use std::{io::ErrorKind, path::PathBuf};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

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

    fn localhost_image(image: &str) -> String {
        image.replacen("127.0.0.1", "localhost", 1)
    }

    async fn collect_events(
        mut events: BoxStream<'static, anyhow::Result<LockUpdateEvent>>
    ) -> anyhow::Result<Vec<LockUpdateEvent>> {
        let mut collected = Vec::new();
        while let Some(event) = events.next().await {
            collected.push(event?);
        }
        Ok(collected)
    }

    fn written_lock_file(
        lock_file: impl AsRef<std::path::Path>
    ) -> anyhow::Result<LockedImages> {
        serde_json::from_str(&std::fs::read_to_string(lock_file)?)
            .map_err(Into::into)
    }

    #[tokio::test]
    async fn no_images_reports_no_images_without_writing() -> anyhow::Result<()>
    {
        let dir = tempfile::tempdir()?;
        let lock_file = dir.path().join("nirion.lock");
        let mut events = image_update_stream(
            &context(
                NirionOciClient::builder().build(),
                LockedImages::default(),
                lock_file.clone(),
            ),
            BTreeMap::new(),
            1,
        );

        assert!(matches!(
            events.next().await.transpose()?,
            Some(LockUpdateEvent::NoImages)
        ));

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
        let events = collect_events(image_update_stream(
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
        ))
        .await?;

        assert!(
            events.iter().any(|event| matches!(
                event,
                LockUpdateEvent::ChangesDetected { diffs }
                    if matches!(diffs.as_slice(), [DiffEntry::Added { service, new }] if service == "app.web" && new.digest == test_image.digest)
            ))
        );

        let written = written_lock_file(lock_file)?;
        assert_eq!(written.get("app.web").unwrap().digest, test_image.digest);
        assert_eq!(
            written.get("app.web").unwrap().image,
            test_image.reference.to_string()
        );

        Ok(())
    }

    #[tokio::test]
    async fn new_image_preserves_configured_image_string() -> anyhow::Result<()>
    {
        let handle = RegistryHandle::start_anonymous().await?;
        let test_image = handle
            .push_anonymous("nirion-lock-update-preserve", "1.2.3")
            .await?;
        let configured_image =
            localhost_image(&test_image.reference.to_string());
        let dir = tempfile::tempdir()?;
        let lock_file = dir.path().join("nirion.lock");

        collect_events(image_update_stream(
            &context(
                http_nirion_client().build(),
                LockedImages::default(),
                lock_file.clone(),
            ),
            BTreeMap::from([("app.web".to_string(), configured_image.clone())]),
            1,
        ))
        .await?;

        let written = written_lock_file(lock_file)?;
        assert_eq!(written.get("app.web").unwrap().image, configured_image);

        Ok(())
    }

    #[tokio::test]
    async fn version_lookup_failure_emits_warning_and_continues()
    -> anyhow::Result<()> {
        let handle = RegistryHandle::start_anonymous().await?;
        let test_image = handle
            .push(
                "library/nirion-lock-update-warning",
                "latest",
                &RegistryAuth::Anonymous,
            )
            .await?;
        let (hub_base_url, hub_server) = start_failing_docker_hub(
            "/namespaces/library/repositories/nirion-lock-update-warning/tags?page_size=100&page=1",
        )
        .await?;
        let docker_hub = DockerHubClient::with_base_url(hub_base_url)
            .with_registries([test_image.registry_addr.clone()]);
        let dir = tempfile::tempdir()?;
        let lock_file = dir.path().join("nirion.lock");

        let events = collect_events(image_update_stream(
            &context(
                http_nirion_client()
                    .docker_hub_client(docker_hub)
                    .build(),
                LockedImages::default(),
                lock_file.clone(),
            ),
            BTreeMap::from([(
                "app.web".to_string(),
                test_image.reference.to_string(),
            )]),
            1,
        ))
        .await?;

        assert!(events.iter().any(|event| matches!(
            event,
            LockUpdateEvent::Warning { service, message }
                if service == "app.web" && message.contains("Failed to resolve version tag")
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            LockUpdateEvent::ChangesDetected { diffs }
                if matches!(diffs.as_slice(), [DiffEntry::Added { service, new }] if service == "app.web" && new.version.is_none() && new.digest == test_image.digest)
        )));

        let written = written_lock_file(lock_file)?;
        let locked = written.get("app.web").unwrap();
        assert_eq!(locked.digest, test_image.digest);
        assert_eq!(locked.version, None);

        hub_server.await??;

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

        let events = collect_events(image_update_stream(
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
        ))
        .await?;

        assert!(
            events
                .iter()
                .any(|event| matches!(event, LockUpdateEvent::UpToDate))
        );
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

        let events = collect_events(image_update_stream(
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
        ))
        .await?;

        assert!(
            events.iter().any(|event| matches!(
                event,
                LockUpdateEvent::ChangesDetected { diffs }
                    if matches!(diffs.as_slice(), [DiffEntry::Updated { service, new, .. }] if service == "app.web" && new.digest == test_image.digest)
            ))
        );
        let written = written_lock_file(lock_file)?;
        assert_eq!(written.get("app.web").unwrap().digest, test_image.digest);
        assert_eq!(
            written.get("app.web").unwrap().image,
            test_image.reference.to_string()
        );

        Ok(())
    }

    #[tokio::test]
    async fn unchanged_digest_with_changed_image_string_writes_lock_file()
    -> anyhow::Result<()> {
        let handle = RegistryHandle::start_anonymous().await?;
        let test_image = handle
            .push_anonymous("nirion-lock-update-image-change", "1.2.3")
            .await?;
        let configured_image =
            localhost_image(&test_image.reference.to_string());
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

        let events = collect_events(image_update_stream(
            &context(
                http_nirion_client().build(),
                locked_images,
                lock_file.clone(),
            ),
            BTreeMap::from([("app.web".to_string(), configured_image.clone())]),
            1,
        ))
        .await?;

        assert!(
            events.iter().any(|event| matches!(
                event,
                LockUpdateEvent::ChangesDetected { diffs }
                    if matches!(diffs.as_slice(), [DiffEntry::Updated { service, new, .. }] if service == "app.web" && new.digest == test_image.digest)
            ))
        );
        let written = written_lock_file(lock_file)?;
        assert_eq!(written.get("app.web").unwrap().image, configured_image);

        Ok(())
    }

    #[tokio::test]
    async fn invalid_image_reference_returns_error() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let lock_file = dir.path().join("nirion.lock");
        let result = collect_events(image_update_stream(
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
        ))
        .await;

        let err = match result {
            Ok(_) => panic!("expected invalid image reference to fail"),
            Err(err) => err,
        };

        assert!(!err.to_string().is_empty());
        assert!(!lock_file.exists());

        Ok(())
    }

    async fn start_failing_docker_hub(
        expected_target: &'static str
    ) -> anyhow::Result<(String, tokio::task::JoinHandle<anyhow::Result<()>>)>
    {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;

        let server = tokio::spawn(async move {
            serve_http_response(
                &listener,
                500,
                r#"{"detail":"nope","message":"failed"}"#,
                expected_target,
            )
            .await?;
            Ok(())
        });

        Ok((format!("http://{addr}"), server))
    }

    async fn serve_http_response(
        listener: &TcpListener,
        status: u16,
        body: &str,
        expected_target: &str,
    ) -> anyhow::Result<()> {
        let (mut socket, _) = listener.accept().await?;
        let mut request = vec![0; 4096];
        let read = socket.read(&mut request).await?;

        if read == 0 {
            return Err(std::io::Error::new(
                ErrorKind::UnexpectedEof,
                "mock Docker Hub request was empty",
            )
            .into());
        }

        let request = std::str::from_utf8(&request[..read])?;
        let target = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .ok_or_else(|| {
                anyhow::anyhow!("mock Docker Hub request was invalid")
            })?;
        assert_eq!(target, expected_target);

        let reason = if status == 200 { "OK" } else { "Error" };
        let response = format!(
            "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        socket
            .write_all(response.as_bytes())
            .await?;
        Ok(())
    }
}

async fn get_cached_updated_image(
    client: &NirionOciClient,
    versioned_image: &VersionedImage,
    cache: &Arc<RwLock<HashMap<String, VersionedImage>>>,
) -> anyhow::Result<VersionedImageResolution> {
    let image = versioned_image.image.as_str();

    if let Some(existing) = {
        let locked_cache = cache.read().await;
        locked_cache.get(image).cloned()
    } {
        return Ok(VersionedImageResolution {
            image: existing,
            warnings: Vec::new(),
        });
    }

    let resolved = client
        .get_updated_versioned_image_resolution(versioned_image)
        .await?;

    {
        let mut locked_cache = cache.write().await;
        locked_cache.insert(image.to_string(), resolved.image.clone());
    }

    Ok(resolved)
}
