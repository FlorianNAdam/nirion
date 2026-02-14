use oci_client::{
    config::ConfigFile, secrets::RegistryAuth, Client, Reference,
};

use crate::{
    docker_hub::get_alias_dockerhub_tags,
    oci::{get_alias_oci_tags, get_version_from_config},
    version::canonical_version_tag,
};

pub async fn get_alias_tags(
    client: &Client,
    image: &Reference,
    digest: &str,
) -> anyhow::Result<Vec<String>> {
    match image.registry() {
        "docker.io" => get_alias_dockerhub_tags(image, &digest).await,
        _ => get_alias_oci_tags(client, image, &digest).await,
    }
}

pub async fn get_version_from_tags(
    client: &Client,
    image: &Reference,
    digest: &str,
) -> anyhow::Result<Option<String>> {
    let alias_tags = get_alias_tags(client, image, digest).await?;
    Ok(canonical_version_tag(&alias_tags))
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
