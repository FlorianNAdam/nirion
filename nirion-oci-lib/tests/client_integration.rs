use std::io::ErrorKind;

use nirion_oci_lib::{
    auth::RegistryAuth as NirionRegistryAuth,
    client::{AuthConfig, NirionOciClient, NirionOciClientConfig},
    docker_hub::{DockerHubClient, DockerHubError},
    oci_client::{
        Client, Reference,
        client::{ClientConfig, ClientProtocol, Config, ImageLayer},
        config::ConfigFile,
        secrets::RegistryAuth,
    },
    version::VersionedImage,
};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

struct TestImage {
    _registry: testcontainers::ContainerAsync<GenericImage>,
    registry_addr: String,
    reference: Reference,
    digest: String,
}

const HTPASSWD: &str =
    "testuser:$2y$05$8/q2bfRcX74EuxGf0qOcSuhWDQJXrgWiy6Fi73/JM2tKC66qSrLve";
const HTPASSWD_USERNAME: &str = "testuser";
const HTPASSWD_PASSWORD: &str = "testpassword";

#[tokio::test]
async fn resolves_local_registry_image_with_mocked_docker_hub_metadata()
-> anyhow::Result<()> {
    let Some(test_image) = push_test_image("latest").await? else {
        return Ok(());
    };

    let (hub_base_url, hub_server) =
        start_mock_docker_hub(&test_image.digest).await?;
    let docker_hub = DockerHubClient::with_base_url(hub_base_url)
        .with_registries([test_image.registry_addr.clone()]);

    let client = http_nirion_client()
        .docker_hub(docker_hub)
        .build();

    let resolved = client
        .get_versioned_image(&test_image.reference)
        .await?;

    assert_eq!(resolved.image, test_image.reference.to_string());
    assert_eq!(resolved.digest, test_image.digest);
    assert_eq!(resolved.version.as_deref(), Some("1.2.3"));

    hub_server.await??;

    Ok(())
}

#[tokio::test]
async fn resolves_local_registry_image_with_generic_oci_tags()
-> anyhow::Result<()> {
    let Some(test_image) = push_test_image("1.2.3").await? else {
        return Ok(());
    };

    let client = http_nirion_client().build();

    let resolved = client
        .get_versioned_image(&test_image.reference)
        .await?;

    assert_eq!(resolved.image, test_image.reference.to_string());
    assert_eq!(resolved.digest, test_image.digest);
    assert_eq!(resolved.version.as_deref(), Some("1.2.3"));

    Ok(())
}

#[tokio::test]
async fn updated_image_preserves_version_when_digest_is_unchanged()
-> anyhow::Result<()> {
    let Some(test_image) = push_test_image("1.2.3").await? else {
        return Ok(());
    };

    let client = http_nirion_client().build();
    let current = VersionedImage {
        image: test_image.reference.to_string(),
        version: Some("1.2.3".to_string()),
        digest: test_image.digest.clone(),
    };

    let resolved = client
        .get_updated_versioned_image(&current)
        .await?;

    assert_eq!(resolved, current);

    Ok(())
}

#[tokio::test]
async fn updated_image_resolves_version_when_digest_changes()
-> anyhow::Result<()> {
    let Some(test_image) = push_test_image("1.2.3").await? else {
        return Ok(());
    };

    let client = http_nirion_client().build();
    let current = VersionedImage {
        image: test_image.reference.to_string(),
        version: Some("1.0.0".to_string()),
        digest: "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_string(),
    };

    let resolved = client
        .get_updated_versioned_image(&current)
        .await?;

    assert_eq!(resolved.image, test_image.reference.to_string());
    assert_eq!(resolved.digest, test_image.digest);
    assert_eq!(resolved.version.as_deref(), Some("1.2.3"));

    Ok(())
}

#[tokio::test]
async fn resolves_authenticated_registry_with_scoped_auth() -> anyhow::Result<()>
{
    let Some(test_image) =
        push_authenticated_test_image("org-a/nirion-test", "1.2.3").await?
    else {
        return Ok(());
    };

    let mut auth = AuthConfig::default();
    auth.add_auth(
        format!("{}/org-a", test_image.registry_addr),
        NirionRegistryAuth::basic(HTPASSWD_USERNAME, HTPASSWD_PASSWORD),
    );

    let client = http_nirion_client().auth(auth).build();
    let resolved = client
        .get_versioned_image(&test_image.reference)
        .await?;

    assert_eq!(resolved.image, test_image.reference.to_string());
    assert_eq!(resolved.digest, test_image.digest);
    assert_eq!(resolved.version.as_deref(), Some("1.2.3"));

    Ok(())
}

#[tokio::test]
async fn docker_hub_client_follows_pagination() -> anyhow::Result<()> {
    let (base_url, server) = start_paginated_mock_docker_hub().await?;
    let reference = Reference::try_from("localhost:5000/nirion-test:latest")?;
    let client = DockerHubClient::with_base_url(base_url)
        .with_registries(["localhost:5000".to_string()]);

    let tags = client
        .fetch_all_tags(&reference, 1)
        .await?;

    assert_eq!(tags.count, 2);
    assert_eq!(tags.results.len(), 2);
    assert_eq!(tags.results[0].name, "latest");
    assert_eq!(tags.results[1].name, "1.2.3");

    server.await??;

    Ok(())
}

#[tokio::test]
async fn docker_hub_client_parses_api_errors() -> anyhow::Result<()> {
    let (base_url, server) = start_error_mock_docker_hub().await?;
    let reference = Reference::try_from("localhost:5000/nirion-test:latest")?;
    let client = DockerHubClient::with_base_url(base_url)
        .with_registries(["localhost:5000".to_string()]);

    let err = client
        .fetch_all_tags(&reference, 100)
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        DockerHubError::Api {
            detail: Some(_),
            message: Some(_),
        }
    ));

    server.await??;

    Ok(())
}

async fn push_test_image(tag: &str) -> anyhow::Result<Option<TestImage>> {
    let registry = match registry_image().start().await {
        Ok(registry) => registry,
        Err(err) => {
            eprintln!("skipping Docker-backed OCI integration test: {err}");
            return Ok(None);
        }
    };

    push_image_to_registry(
        registry,
        "library/nirion-test",
        tag,
        &RegistryAuth::Anonymous,
    )
    .await
}

async fn push_authenticated_test_image(
    repository: &str,
    tag: &str,
) -> anyhow::Result<Option<TestImage>> {
    let registry = match registry_image()
        .with_env_var("REGISTRY_AUTH", "htpasswd")
        .with_env_var("REGISTRY_AUTH_HTPASSWD_REALM", "Registry Realm")
        .with_env_var("REGISTRY_AUTH_HTPASSWD_PATH", "/auth/htpasswd")
        .with_copy_to("/auth/htpasswd", HTPASSWD.as_bytes().to_vec())
        .start()
        .await
    {
        Ok(registry) => registry,
        Err(err) => {
            eprintln!(
                "skipping Docker-backed authenticated OCI integration test: {err}"
            );
            return Ok(None);
        }
    };

    push_image_to_registry(
        registry,
        repository,
        tag,
        &RegistryAuth::Basic(
            HTPASSWD_USERNAME.to_string(),
            HTPASSWD_PASSWORD.to_string(),
        ),
    )
    .await
}

fn registry_image() -> GenericImage {
    GenericImage::new("registry", "3")
        .with_exposed_port(5000.tcp())
        .with_wait_for(WaitFor::message_on_stderr("listening on"))
}

async fn push_image_to_registry(
    registry: testcontainers::ContainerAsync<GenericImage>,
    repository: &str,
    tag: &str,
    auth: &RegistryAuth,
) -> anyhow::Result<Option<TestImage>> {
    let registry_port = registry
        .get_host_port_ipv4(5000.tcp())
        .await?;
    let registry_addr = format!("127.0.0.1:{registry_port}");
    let image = format!("{registry_addr}/{repository}:{tag}");
    let reference = Reference::try_from(image.as_str())?;

    let oci_client = Client::new(ClientConfig {
        protocol: ClientProtocol::Http,
        ..Default::default()
    });

    let layers = [ImageLayer::oci_v1(b"nirion-test-layer".to_vec(), None)];
    let config = Config::oci_v1_from_config_file(ConfigFile::default(), None)?;
    oci_client
        .push(&reference, &layers, config, auth, None)
        .await?;

    let (_, digest, _) = oci_client
        .pull_manifest_and_config(&reference, auth)
        .await?;

    Ok(Some(TestImage {
        _registry: registry,
        registry_addr,
        reference,
        digest,
    }))
}

fn http_nirion_client() -> nirion_oci_lib::client::NirionOciClientBuilder {
    NirionOciClient::builder()
        .auth(AuthConfig::default())
        .oci_client_config(NirionOciClientConfig {
            protocol: ClientProtocol::Http,
            ..Default::default()
        })
}

async fn start_mock_docker_hub(
    digest: &str,
) -> anyhow::Result<(String, tokio::task::JoinHandle<anyhow::Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let body = docker_hub_tags_response(digest);

    let server = tokio::spawn(async move {
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

        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        socket
            .write_all(response.as_bytes())
            .await?;
        Ok(())
    });

    Ok((format!("http://{addr}"), server))
}

async fn start_paginated_mock_docker_hub()
-> anyhow::Result<(String, tokio::task::JoinHandle<anyhow::Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let second_url = format!(
        "http://{addr}/repositories/library/nirion-test/tags?page_size=1&page=2"
    );
    let first = docker_hub_tags_page(Some(&second_url), &["latest"]);
    let second = docker_hub_tags_page(None, &["1.2.3"]);

    let server = tokio::spawn(async move {
        serve_http_response(&listener, 200, &first).await?;
        serve_http_response(&listener, 200, &second).await?;
        Ok(())
    });

    Ok((format!("http://{addr}"), server))
}

async fn start_error_mock_docker_hub()
-> anyhow::Result<(String, tokio::task::JoinHandle<anyhow::Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let body = r#"{"detail":"nope","message":"failed"}"#.to_string();

    let server = tokio::spawn(async move {
        serve_http_response(&listener, 500, &body).await?;
        Ok(())
    });

    Ok((format!("http://{addr}"), server))
}

async fn serve_http_response(
    listener: &TcpListener,
    status: u16,
    body: &str,
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

fn docker_hub_tags_response(digest: &str) -> String {
    let arch =
        nirion_oci_lib::oci_client::config::Architecture::default().to_string();
    format!(
        r#"{{
  "count": 2,
  "next": null,
  "previous": null,
  "results": [
    {tag_latest},
    {tag_version}
  ]
}}"#,
        tag_latest = docker_hub_tag("latest", &arch, digest),
        tag_version = docker_hub_tag("1.2.3", &arch, digest),
    )
}

fn docker_hub_tags_page(next: Option<&str>, names: &[&str]) -> String {
    let arch =
        nirion_oci_lib::oci_client::config::Architecture::default().to_string();
    let digest = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let results = names
        .iter()
        .map(|name| docker_hub_tag(name, &arch, digest))
        .collect::<Vec<_>>()
        .join(",");
    let next = next
        .map(|url| format!("\"{url}\""))
        .unwrap_or_else(|| "null".to_string());

    format!(
        r#"{{
  "count": 2,
  "next": {next},
  "previous": null,
  "results": [{results}]
}}"#,
    )
}

fn docker_hub_tag(name: &str, arch: &str, digest: &str) -> String {
    format!(
        r#"{{
      "id": 0,
      "images": [{image}],
      "creator": 0,
      "last_updated": null,
      "last_updater": 0,
      "last_updater_username": "",
      "name": "{name}",
      "repository": 0,
      "full_size": 0,
      "v2": true,
      "status": null,
      "tag_last_pulled": null,
      "tag_last_pushed": null
    }}"#,
        image = docker_hub_image(arch, digest),
    )
}

fn docker_hub_image(arch: &str, digest: &str) -> String {
    format!(
        r#"{{
        "architecture": "{arch}",
        "features": "",
        "variant": null,
        "digest": "{digest}",
        "layers": null,
        "os": "linux",
        "os_features": "",
        "os_version": null,
        "size": 0,
        "status": "active",
        "last_pulled": null,
        "last_pushed": null
      }}"#,
    )
}
