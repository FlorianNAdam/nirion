use nirion_oci_lib::unified::get_version_and_digest;
use oci_client::{Client, Reference};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    const DOCKER_IMAGES: &[&str] = &[
        // "ghcr.io/atuinsh/atuin:latest",
        "postgres:14",
        "ghcr.io/goauthentik/server:2025.10.2",
        "ghcr.io/goauthentik/server:latest",
        "postgres:16-alpine",
        "redis:alpine",
        "authelia/authelia",
        "nginx:alpine",
        "traefik/whoami",
        "henrygd/beszel:latest",
        "henrygd/beszel-agent:latest",
        "crowdsecurity/crowdsec:latest",
        "fbonalair/traefik-crowdsec-bouncer:latest",
        "ghcr.io/theduffman85/crowdsec-web-ui:latest",
        "wardy784/erugo:latest",
        "ghcr.io/immich-app/immich-server:v2.4.1",
        "ghcr.io/immich-app/postgres:14-vectorchord0.4.3-pgvectors0.2.0",
        "docker.io/valkey/valkey:8-bookworm",
        "oznu/cloudflare-ddns:latest",
        "crazymax/diun:latest",
        "ghcr.io/analogj/scrutiny:master-omnibus",
        "traefik:latest",
        "binwiederhier/ntfy:latest",
        "vaultwarden/server:latest",
        "seafileltd/seafile-mc:12.0-latest",
        "mariadb:10.11",
        "memcached:1.6.18",
        "shlinkio/shlink:stable",
        "shlinkio/shlink-web-client",
        "ghcr.io/floriannadam/story_tracker-icloud_sync:latest",
        "ghcr.io/floriannadam/story_tracker_v2:latest",
        "instrumentisto/geckodriver",
        "c4illin/convertx",
        "jgraph/drawio",
        "corentinth/it-tools:latest",
        "stirlingtools/stirling-pdf:latest",
        "ghcr.io/floriannadam/recorder:latest",
    ];

    let client = Client::default();
    client
        .store_auth_if_needed(
            "index.docker.io",
            &oci_client::secrets::RegistryAuth::Basic(
                std::env::var("DOCKER_USER")?,
                std::env::var("DOCKER_TOKEN")?,
            ),
        )
        .await;
    client
        .store_auth_if_needed(
            "ghcr.io",
            &oci_client::secrets::RegistryAuth::Basic(
                std::env::var("GHCR_USER")?,
                std::env::var("GHCR_TOKEN")?,
            ),
        )
        .await;

    for image in DOCKER_IMAGES {
        let reference = Reference::try_from(*image)?;
        println!("{}", reference);

        let (version, digest) =
            get_version_and_digest(&client, &reference).await?;

        println!(
            "version: {}",
            version.unwrap_or_else(|| "<unknown>".to_string())
        );
        println!("digest: {}", digest);
        println!()
    }

    Ok(())
}
