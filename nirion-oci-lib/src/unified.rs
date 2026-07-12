use std::str::FromStr;

use oci_client::{
    Client, Reference, config::ConfigFile, secrets::RegistryAuth,
};

use crate::{
    docker_hub::DockerHubClient,
    oci::{
        get_alias_oci_tags, get_version_from_config, get_version_from_oci_tags,
    },
    version::{VersionedImage, canonical_version_tag},
};

pub async fn get_alias_tags_with_auth(
    client: &Client,
    docker_hub: &DockerHubClient,
    image: &Reference,
    digest: &str,
    auth: &RegistryAuth,
) -> anyhow::Result<Vec<String>> {
    if docker_hub.supports(image) {
        docker_hub
            .get_alias_tags(image, digest)
            .await
    } else {
        get_alias_oci_tags(client, image, digest, auth).await
    }
}

pub async fn get_version_from_tags_with_auth(
    client: &Client,
    docker_hub: &DockerHubClient,
    image: &Reference,
    digest: &str,
    auth: &RegistryAuth,
) -> anyhow::Result<Option<String>> {
    if docker_hub.supports(image) {
        let alias_tags =
            get_alias_tags_with_auth(client, docker_hub, image, digest, auth)
                .await?;
        Ok(canonical_version_tag(&alias_tags))
    } else {
        get_version_from_oci_tags(client, image, digest, auth).await
    }
}

pub async fn get_version_and_digest_with_auth(
    client: &Client,
    docker_hub: &DockerHubClient,
    image: &Reference,
    auth: &RegistryAuth,
) -> anyhow::Result<(Option<String>, String)> {
    let (_, digest, raw_config) = client
        .pull_manifest_and_config(image, auth)
        .await?;

    let config: ConfigFile = serde_json::from_str(&raw_config)?;

    if let Some(version) = get_version_from_config(&config) {
        return Ok((Some(version), digest));
    }

    if let Some(version) = get_version_from_tags_with_auth(
        client, docker_hub, image, &digest, auth,
    )
    .await?
    {
        return Ok((Some(version), digest));
    }

    Ok((None, digest))
}

pub async fn get_versioned_image_with_auth(
    client: &Client,
    docker_hub: &DockerHubClient,
    image: &Reference,
    auth: &RegistryAuth,
) -> anyhow::Result<VersionedImage> {
    let (version, digest) =
        get_version_and_digest_with_auth(client, docker_hub, image, auth)
            .await?;
    Ok(VersionedImage {
        image: image.to_string(),
        version,
        digest,
    })
}

pub async fn get_updated_version_and_digest_with_auth(
    client: &Client,
    docker_hub: &DockerHubClient,
    versioned_image: &VersionedImage,
    auth: &RegistryAuth,
) -> anyhow::Result<(Option<String>, String)> {
    let image = Reference::from_str(&versioned_image.image)?;

    let (_, digest, raw_config) = client
        .pull_manifest_and_config(&image, auth)
        .await?;

    if digest == versioned_image.digest {
        return Ok((
            versioned_image.version.clone(),
            versioned_image.digest.to_string(),
        ));
    }

    let config: ConfigFile = serde_json::from_str(&raw_config)?;

    if let Some(version) = get_version_from_config(&config) {
        return Ok((Some(version), digest));
    }

    if let Some(version) = get_version_from_tags_with_auth(
        client, docker_hub, &image, &digest, auth,
    )
    .await?
    {
        return Ok((Some(version), digest));
    }

    Ok((None, digest))
}

pub async fn get_updated_versioned_image_with_auth(
    client: &Client,
    docker_hub: &DockerHubClient,
    versioned_image: &VersionedImage,
    auth: &RegistryAuth,
) -> anyhow::Result<VersionedImage> {
    let (version, digest) = get_updated_version_and_digest_with_auth(
        client,
        docker_hub,
        versioned_image,
        auth,
    )
    .await?;
    Ok(VersionedImage {
        image: versioned_image.image.to_string(),
        version,
        digest,
    })
}

pub async fn get_alias_tags(
    client: &Client,
    image: &Reference,
    digest: &str,
) -> anyhow::Result<Vec<String>> {
    match image.registry() {
        "docker.io" => {
            DockerHubClient::default()
                .get_alias_tags(image, digest)
                .await
        }
        _ => {
            get_alias_oci_tags(client, image, &digest, &RegistryAuth::Anonymous)
                .await
        }
    }
}

pub async fn get_version_from_tags(
    client: &Client,
    image: &Reference,
    digest: &str,
) -> anyhow::Result<Option<String>> {
    match image.registry() {
        "docker.io" => {
            let alias_tags = get_alias_tags(client, image, digest).await?;
            let version_tag = canonical_version_tag(&alias_tags);
            return Ok(version_tag);
        }
        _ => {
            get_version_from_oci_tags(
                client,
                image,
                digest,
                &RegistryAuth::Anonymous,
            )
            .await
        }
    }
}

pub async fn get_version_and_digest(
    client: &Client,
    image: &Reference,
) -> anyhow::Result<(Option<String>, String)> {
    let auth = RegistryAuth::Anonymous;
    let (_, digest, raw_config) = client
        .pull_manifest_and_config(&image, &auth)
        .await?;

    let config: ConfigFile = serde_json::from_str(&raw_config)?;

    if let Some(version) = get_version_from_config(&config) {
        return Ok((Some(version), digest));
    }

    if let Some(version) = get_version_from_tags(client, image, &digest).await?
    {
        return Ok((Some(version), digest));
    }

    Ok((None, digest))
}

pub async fn get_version(
    client: &Client,
    image: &Reference,
) -> anyhow::Result<Option<String>> {
    let (version, _) = get_version_and_digest(client, image).await?;
    Ok(version)
}

pub async fn get_versioned_image(
    client: &Client,
    image: &Reference,
) -> anyhow::Result<VersionedImage> {
    let (version, digest) = get_version_and_digest(client, image).await?;
    Ok(VersionedImage {
        image: image.to_string(),
        version,
        digest,
    })
}

pub async fn get_updated_version_and_digest(
    client: &Client,
    versioned_image: &VersionedImage,
) -> anyhow::Result<(Option<String>, String)> {
    let image = Reference::from_str(&versioned_image.image)?;

    let auth = RegistryAuth::Anonymous;

    let (_, digest, raw_config) = client
        .pull_manifest_and_config(&image, &auth)
        .await?;

    if digest == versioned_image.digest {
        return Ok((
            versioned_image.version.clone(),
            versioned_image.digest.to_string(),
        ));
    }

    let config: ConfigFile = serde_json::from_str(&raw_config)?;

    if let Some(version) = get_version_from_config(&config) {
        return Ok((Some(version), digest));
    }

    if let Some(version) =
        get_version_from_tags(client, &image, &digest).await?
    {
        return Ok((Some(version), digest));
    }

    Ok((None, digest))
}

pub async fn get_updated_version(
    client: &Client,
    versioned_image: &VersionedImage,
) -> anyhow::Result<Option<String>> {
    let (version, _) =
        get_updated_version_and_digest(client, versioned_image).await?;
    Ok(version)
}

pub async fn get_updated_versioned_image(
    client: &Client,
    versioned_image: &VersionedImage,
) -> anyhow::Result<VersionedImage> {
    let (version, digest) =
        get_updated_version_and_digest(client, versioned_image).await?;
    Ok(VersionedImage {
        image: versioned_image.image.to_string(),
        version,
        digest,
    })
}
