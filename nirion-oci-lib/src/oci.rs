use oci_client::{
    config::{Architecture, ConfigFile},
    manifest::OciManifest,
    secrets::RegistryAuth,
    Client, Reference,
};

use crate::version::{canonical_version_score, clean_tag, NON_VERSION_TAGS};

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
        .map(|t| clean_tag(t).to_string())
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
    let page_size = 1000; // reasonable default
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

pub async fn get_version_from_oci_tags(
    client: &Client,
    image: &Reference,
    digest: &str,
) -> anyhow::Result<Option<String>> {
    let tags = list_all_tags(&client, image).await?;

    let mut tags = tags
        .into_iter()
        .filter(|version| !NON_VERSION_TAGS.contains(&version.as_str()))
        .collect::<Vec<_>>();

    tags.sort_by_cached_key(|tag| {
        let clean_tag = clean_tag(tag);
        canonical_version_score(clean_tag)
    });

    for tag in tags.into_iter().rev() {
        let tag_reference = Reference::with_tag(
            image.registry().to_string(),
            image.repository().to_string(),
            tag.clone(),
        );

        let tag_digest = pull_platform_digest(&client, &tag_reference).await?;
        if tag_digest == digest {
            let clean_tag = clean_tag(&tag).to_string();
            return Ok(Some(clean_tag));
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use oci_client::{
        config::{Architecture, Config, ConfigFile, Os},
        manifest::{
            ImageIndexEntry, OciImageIndex, OciImageManifest, OciManifest,
            Platform,
        },
    };

    fn config_with_version(version: Option<&str>) -> ConfigFile {
        let labels = version.map(|version| {
            HashMap::from([(
                "org.opencontainers.image.version".to_string(),
                version.to_string(),
            )])
        });

        ConfigFile {
            config: Some(Config {
                labels,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn resolve_registry_normalizes_docker_hub() {
        assert_eq!(
            resolve_registry("docker.io".to_string()),
            "index.docker.io"
        );
        assert_eq!(resolve_registry("ghcr.io".to_string()), "ghcr.io");
    }

    #[test]
    fn get_version_from_config_extracts_and_cleans_label() {
        let config =
            config_with_version(Some("refs/tags/version/1.2.3-bookworm"));

        assert_eq!(get_version_from_config(&config), Some("1.2.3".to_string()));
    }

    #[test]
    fn get_version_from_config_ignores_missing_and_non_version_labels() {
        assert_eq!(get_version_from_config(&ConfigFile::default()), None);
        assert_eq!(get_version_from_config(&config_with_version(None)), None);
        assert_eq!(
            get_version_from_config(&config_with_version(Some("latest"))),
            None
        );
    }

    #[test]
    fn get_digest_from_manifest_returns_manifest_digest_for_image_manifest() {
        let manifest = OciManifest::Image(OciImageManifest::default());

        assert_eq!(
            get_digest_from_manifest("sha256:abc", &manifest).unwrap(),
            "sha256:abc"
        );
    }

    #[test]
    fn get_digest_from_manifest_returns_platform_digest_for_image_index() {
        let manifest = OciManifest::ImageIndex(OciImageIndex {
            schema_version: 2,
            media_type: None,
            manifests: vec![ImageIndexEntry {
                media_type: String::new(),
                digest: "sha256:platform".to_string(),
                size: 0,
                platform: Some(Platform {
                    architecture: Architecture::default(),
                    os: Os::default(),
                    os_version: None,
                    os_features: None,
                    variant: None,
                    features: None,
                }),
                annotations: None,
            }],
            artifact_type: None,
            annotations: None,
        });

        assert_eq!(
            get_digest_from_manifest("sha256:index", &manifest).unwrap(),
            "sha256:platform"
        );
    }

    #[test]
    fn get_digest_from_manifest_errors_when_image_index_has_no_matching_platform(
    ) {
        let manifest = OciManifest::ImageIndex(OciImageIndex {
            schema_version: 2,
            media_type: None,
            manifests: vec![ImageIndexEntry {
                media_type: String::new(),
                digest: "sha256:other".to_string(),
                size: 0,
                platform: None,
                annotations: None,
            }],
            artifact_type: None,
            annotations: None,
        });

        let err =
            get_digest_from_manifest("sha256:index", &manifest).unwrap_err();

        assert!(err
            .to_string()
            .contains("No matching platform found"));
    }
}
