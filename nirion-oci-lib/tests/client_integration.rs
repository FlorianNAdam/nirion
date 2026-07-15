use std::io::ErrorKind;

use nirion_oci_lib::{
    auth::RegistryAuth as NirionRegistryAuth,
    client::AuthConfig,
    docker_hub::{DockerHubClient, DockerHubError},
    oci::get_alias_oci_tags,
    oci_client::{
        Client, Reference,
        client::{ClientConfig, ClientProtocol},
        secrets::RegistryAuth,
    },
    test_registry::{
        ACCOUNT_A, ACCOUNT_B, RegistryHandle, http_nirion_client,
        push_anonymous_test_image,
    },
    version::VersionedImage,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[tokio::test]
async fn resolves_local_registry_image_with_mocked_docker_hub_metadata()
-> anyhow::Result<()> {
    let (_handle, test_image) = push_anonymous_test_image("latest").await?;

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
    let (_handle, test_image) = push_anonymous_test_image("1.2.3").await?;

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
    let (_handle, test_image) = push_anonymous_test_image("1.2.3").await?;

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
    let (_handle, test_image) = push_anonymous_test_image("1.2.3").await?;

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
async fn resolves_repository_scoped_auth() -> anyhow::Result<()> {
    let handle = RegistryHandle::start(&[ACCOUNT_A]).await?;

    let test_image = handle
        .push(
            "org-a/nirion-test",
            "1.2.3",
            &RegistryAuth::Basic(
                ACCOUNT_A.username.to_string(),
                ACCOUNT_A.password.to_string(),
            ),
        )
        .await?;

    let mut auth = AuthConfig::default();
    auth.add_auth(
        format!("{}/org-a", test_image.registry_addr),
        NirionRegistryAuth::basic(ACCOUNT_A.username, ACCOUNT_A.password),
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
async fn falls_back_to_registry_scoped_auth() -> anyhow::Result<()> {
    let handle = RegistryHandle::start(&[ACCOUNT_A]).await?;

    let test_image = handle
        .push(
            "org-a/nirion-test",
            "1.2.3",
            &RegistryAuth::Basic(
                ACCOUNT_A.username.to_string(),
                ACCOUNT_A.password.to_string(),
            ),
        )
        .await?;

    let mut auth = AuthConfig::default();
    auth.add_auth(
        test_image.registry_addr.clone(),
        NirionRegistryAuth::basic(ACCOUNT_A.username, ACCOUNT_A.password),
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
async fn different_scopes_on_same_registry() -> anyhow::Result<()> {
    let handle = RegistryHandle::start(&[ACCOUNT_A, ACCOUNT_B]).await?;

    let image_a = handle
        .push(
            "org-a/nirion-test",
            "1.0.0",
            &RegistryAuth::Basic(
                ACCOUNT_A.username.to_string(),
                ACCOUNT_A.password.to_string(),
            ),
        )
        .await?;

    let image_b = handle
        .push(
            "org-b/nirion-test",
            "2.0.0",
            &RegistryAuth::Basic(
                ACCOUNT_B.username.to_string(),
                ACCOUNT_B.password.to_string(),
            ),
        )
        .await?;

    let mut auth = AuthConfig::default();
    auth.add_auth(
        format!("{}/org-a", image_a.registry_addr),
        NirionRegistryAuth::basic(ACCOUNT_A.username, ACCOUNT_A.password),
    );
    auth.add_auth(
        format!("{}/org-b", image_b.registry_addr),
        NirionRegistryAuth::basic(ACCOUNT_B.username, ACCOUNT_B.password),
    );

    let client = http_nirion_client().auth(auth).build();

    let resolved_a = client
        .get_versioned_image(&image_a.reference)
        .await?;
    assert_eq!(resolved_a.digest, image_a.digest);

    let resolved_b = client
        .get_versioned_image(&image_b.reference)
        .await?;
    assert_eq!(resolved_b.digest, image_b.digest);

    Ok(())
}

#[tokio::test]
async fn scoped_auth_isolation_across_registries() -> anyhow::Result<()> {
    let handle_a = RegistryHandle::start(&[ACCOUNT_A]).await?;
    let handle_b = RegistryHandle::start(&[ACCOUNT_B]).await?;

    let image_a = handle_a
        .push(
            "org/nirion-test",
            "1.0.0",
            &RegistryAuth::Basic(
                ACCOUNT_A.username.to_string(),
                ACCOUNT_A.password.to_string(),
            ),
        )
        .await?;

    let image_b = handle_b
        .push(
            "org/nirion-test",
            "2.0.0",
            &RegistryAuth::Basic(
                ACCOUNT_B.username.to_string(),
                ACCOUNT_B.password.to_string(),
            ),
        )
        .await?;

    let mut auth = AuthConfig::default();
    auth.add_auth(
        image_a.registry_addr.clone(),
        NirionRegistryAuth::basic(ACCOUNT_A.username, ACCOUNT_A.password),
    );
    auth.add_auth(
        image_b.registry_addr.clone(),
        NirionRegistryAuth::basic(ACCOUNT_B.username, ACCOUNT_B.password),
    );

    let client = http_nirion_client().auth(auth).build();

    let resolved_a = client
        .get_versioned_image(&image_a.reference)
        .await?;
    assert_eq!(resolved_a.digest, image_a.digest);

    let resolved_b = client
        .get_versioned_image(&image_b.reference)
        .await?;
    assert_eq!(resolved_b.digest, image_b.digest);

    Ok(())
}

#[tokio::test]
async fn updated_image_resolves_version_with_scoped_auth() -> anyhow::Result<()>
{
    let handle = RegistryHandle::start(&[ACCOUNT_A]).await?;

    let test_image = handle
        .push(
            "org-a/nirion-test",
            "1.2.3",
            &RegistryAuth::Basic(
                ACCOUNT_A.username.to_string(),
                ACCOUNT_A.password.to_string(),
            ),
        )
        .await?;

    let mut auth = AuthConfig::default();
    auth.add_auth(
        format!("{}/org-a", test_image.registry_addr),
        NirionRegistryAuth::basic(ACCOUNT_A.username, ACCOUNT_A.password),
    );

    let client = http_nirion_client().auth(auth).build();

    let stale = VersionedImage {
        image: test_image.reference.to_string(),
        version: Some("1.0.0".to_string()),
        digest: "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_string(),
    };

    let resolved = client
        .get_updated_versioned_image(&stale)
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

#[tokio::test]
async fn docker_hub_client_fetches_single_tag() -> anyhow::Result<()> {
    let digest = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let arch =
        nirion_oci_lib::oci_client::config::Architecture::default().to_string();
    let body = docker_hub_tag("1.2.3", &arch, digest);
    let (base_url, server) =
        start_single_response_mock_docker_hub(200, body).await?;
    let reference = Reference::try_from("localhost:5000/nirion-test:1.2.3")?;
    let client = DockerHubClient::with_base_url(base_url)
        .with_registries(["localhost:5000".to_string()]);

    let tag = client.fetch_tag(&reference).await?;

    assert_eq!(tag.name, "1.2.3");
    assert_eq!(tag.images[0].digest.as_deref(), Some(digest));

    server.await??;

    Ok(())
}

#[tokio::test]
async fn docker_hub_client_rejects_digest_references() -> anyhow::Result<()> {
    let client = DockerHubClient::default();
    let digest_reference = Reference::try_from(
        "nginx@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )?;

    assert!(matches!(
        client
            .fetch_tag(&digest_reference)
            .await,
        Err(DockerHubError::DigestNotSupported)
    ));

    Ok(())
}

#[tokio::test]
async fn docker_hub_client_reports_unparseable_error_status()
-> anyhow::Result<()> {
    let (base_url, server) =
        start_single_response_mock_docker_hub(503, "not json".to_string())
            .await?;
    let reference = Reference::try_from("localhost:5000/nirion-test:1.2.3")?;
    let client = DockerHubClient::with_base_url(base_url)
        .with_registries(["localhost:5000".to_string()]);

    let err = client
        .fetch_tag(&reference)
        .await
        .unwrap_err();

    assert!(
        matches!(err, DockerHubError::UnexpectedStatus(status) if status.as_u16() == 503)
    );

    server.await??;

    Ok(())
}

#[tokio::test]
async fn oci_alias_tags_return_tags_with_matching_digest() -> anyhow::Result<()>
{
    let handle = RegistryHandle::start(&[]).await?;

    let latest = handle
        .push(
            "library/nirion-alias-test",
            "latest",
            &RegistryAuth::Anonymous,
        )
        .await?;
    handle
        .push(
            "library/nirion-alias-test",
            "1.2.3",
            &RegistryAuth::Anonymous,
        )
        .await?;

    let client = Client::new(ClientConfig {
        protocol: ClientProtocol::Http,
        ..Default::default()
    });
    let tags = get_alias_oci_tags(
        &client,
        &latest.reference,
        &latest.digest,
        &RegistryAuth::Anonymous,
    )
    .await?;

    assert!(tags.contains(&"latest".to_string()));
    assert!(tags.contains(&"1.2.3".to_string()));

    Ok(())
}

async fn start_mock_docker_hub(
    digest: &str
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

async fn start_single_response_mock_docker_hub(
    status: u16,
    body: String,
) -> anyhow::Result<(String, tokio::task::JoinHandle<anyhow::Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let server = tokio::spawn(async move {
        serve_http_response(&listener, status, &body).await?;
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

fn docker_hub_tags_page(
    next: Option<&str>,
    names: &[&str],
) -> String {
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

fn docker_hub_tag(
    name: &str,
    arch: &str,
    digest: &str,
) -> String {
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

fn docker_hub_image(
    arch: &str,
    digest: &str,
) -> String {
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
