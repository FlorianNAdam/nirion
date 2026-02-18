use oci_client::{
    config::{Architecture, ConfigFile},
    manifest::OciManifest,
    secrets::RegistryAuth,
    Client, Reference,
};

use crate::version::NON_VERSION_TAGS;

pub fn resolve_registry(registry: String) -> String {
    Reference::with_tag(registry, "dummy".to_string(), "dummy".to_string())
        .resolve_registry()
        .to_string()
}

pub fn get_version_from_config(config: &ConfigFile) -> Option<String> {
    let config = config.config.as_ref()?;
    let labels = config.labels.as_ref()?;
    labels
        .get("org.opencontainers.image.version")
        .filter(|version| !NON_VERSION_TAGS.contains(&version.as_str()))
        .cloned()
}

pub async fn get_alias_oci_tags(
    client: &Client,
    image: &Reference,
    digest: &str,
) -> anyhow::Result<Vec<String>> {
    let tags = list_all_tags(&client, image).await?;

    let mut tag_refs = tags
        .into_iter()
        .map(|tag| {
            Reference::with_tag(
                image.registry().to_string(),
                image.repository().to_string(),
                tag.clone(),
            )
        })
        .rev()
        .peekable();

    while let Some(image) = tag_refs.peek() {
        let tag_digest = pull_platform_digest(&client, &image).await?;
        if tag_digest == digest {
            break;
        } else {
            tag_refs.next();
        }
    }

    let mut candidates = Vec::new();

    while let Some(image) = tag_refs.peek() {
        let tag_digest = pull_platform_digest(&client, &image).await?;
        if tag_digest != digest {
            break;
        } else {
            if let Some(tag) = image.tag() {
                candidates.push(tag.to_string());
            }
            tag_refs.next();
        }
    }

    Ok(candidates)
}

pub async fn list_all_tags(
    client: &Client,
    image: &Reference,
) -> anyhow::Result<Vec<String>> {
    let page_size = 100; // reasonable default
    let mut all_tags = Vec::new();
    let mut last: Option<String> = None;

    loop {
        let auth = RegistryAuth::Anonymous;
        let tags = client
            .list_tags(image, &auth, Some(page_size), last.as_deref())
            .await?
            .tags;

        let count = tags.len();
        last = tags.last().cloned();
        all_tags.extend(tags);

        if count < page_size {
            break;
        }
    }

    Ok(all_tags)
}

pub async fn pull_platform_digest(
    client: &Client,
    image: &Reference,
) -> anyhow::Result<String> {
    let auth = RegistryAuth::Anonymous;
    let (manifest, digest) = client
        .pull_manifest(image, &auth)
        .await?;

    get_digest_from_manifest(&digest, &manifest)
}

pub fn get_digest_from_manifest(
    digest: &str,
    manifest: &OciManifest,
) -> anyhow::Result<String> {
    let arch = Architecture::default();

    match manifest {
        OciManifest::Image(_) => Ok(digest.to_string()),
        OciManifest::ImageIndex(index) => {
            let descriptor = index
                .manifests
                .iter()
                .find(|m| {
                    if let Some(platform) = &m.platform {
                        platform.architecture == arch
                    } else {
                        false
                    }
                })
                .ok_or_else(|| anyhow::anyhow!("No matching platform found"))?;
            Ok(descriptor.digest.clone())
        }
    }
}
