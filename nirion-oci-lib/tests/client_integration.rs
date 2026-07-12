use std::io::ErrorKind;

use nirion_oci_lib::{
    client::{AuthConfig, NirionOciClient, NirionOciClientConfig},
    docker_hub::DockerHubClient,
    oci_client::{
        Client, Reference,
        client::{ClientConfig, ClientProtocol, Config, ImageLayer},
        config::ConfigFile,
        secrets::RegistryAuth,
    },
};
use testcontainers::{
    GenericImage,
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

async fn push_test_image(tag: &str) -> anyhow::Result<Option<TestImage>> {
    let registry = match GenericImage::new("registry", "3")
        .with_exposed_port(5000.tcp())
        .with_wait_for(WaitFor::message_on_stderr("listening on"))
        .start()
        .await
    {
        Ok(registry) => registry,
        Err(err) => {
            eprintln!("skipping Docker-backed OCI integration test: {err}");
            return Ok(None);
        }
    };

    let registry_port = registry
        .get_host_port_ipv4(5000.tcp())
        .await?;
    let registry_addr = format!("127.0.0.1:{registry_port}");
    let image = format!("{registry_addr}/library/nirion-test:{tag}");
    let reference = Reference::try_from(image.as_str())?;

    let oci_client = Client::new(ClientConfig {
        protocol: ClientProtocol::Http,
        ..Default::default()
    });
    let auth = RegistryAuth::Anonymous;

    let layers = [ImageLayer::oci_v1(b"nirion-test-layer".to_vec(), None)];
    let config = Config::oci_v1_from_config_file(ConfigFile::default(), None)?;
    oci_client
        .push(&reference, &layers, config, &auth, None)
        .await?;

    let (_, digest, _) = oci_client
        .pull_manifest_and_config(&reference, &auth)
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
