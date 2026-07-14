use crate::{
    client::{AuthConfig, NirionOciClient, NirionOciClientConfig},
    oci_client::{
        Client, Reference,
        client::{ClientConfig, ClientProtocol, Config, ImageLayer},
        config::ConfigFile,
        secrets::RegistryAuth,
    },
};
use testcontainers::{
    ContainerRequest, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};

pub struct TestImage {
    pub registry_addr: String,
    pub reference: Reference,
    pub digest: String,
}

pub struct TestAccount {
    pub username: &'static str,
    pub password: &'static str,
    pub htpasswd_line: &'static str,
}

pub const ACCOUNT_A: TestAccount = TestAccount {
    username: "testuser",
    password: "testpassword",
    htpasswd_line: "testuser:$2y$05$8/q2bfRcX74EuxGf0qOcSuhWDQJXrgWiy6Fi73/JM2tKC66qSrLve",
};

pub const ACCOUNT_B: TestAccount = TestAccount {
    username: "testuser-b",
    password: "testpassword2",
    htpasswd_line: "testuser-b:$2b$05$3xL.QkaFDSxihClnY2VX1OrmHSsnlkkpJLB0zSiv.CWVYAoUJ6Y/u",
};

pub struct RegistryHandle {
    _container: testcontainers::ContainerAsync<GenericImage>,
    pub addr: String,
}

impl RegistryHandle {
    pub async fn start(
        accounts: &[TestAccount]
    ) -> anyhow::Result<Option<Self>> {
        let image = if accounts.is_empty() {
            ContainerRequest::from(registry_image())
        } else {
            let htpasswd = accounts
                .iter()
                .map(|a| a.htpasswd_line)
                .collect::<Vec<_>>()
                .join("\n");
            ContainerRequest::from(registry_image())
                .with_env_var("REGISTRY_AUTH", "htpasswd")
                .with_env_var("REGISTRY_AUTH_HTPASSWD_REALM", "Registry Realm")
                .with_env_var("REGISTRY_AUTH_HTPASSWD_PATH", "/auth/htpasswd")
                .with_copy_to("/auth/htpasswd", htpasswd.into_bytes())
        };

        match image.start().await {
            Ok(container) => {
                let port = container
                    .get_host_port_ipv4(5000.tcp())
                    .await?;
                Ok(Some(Self {
                    _container: container,
                    addr: format!("127.0.0.1:{port}"),
                }))
            }
            Err(err) => {
                eprintln!("skipping Docker-backed OCI integration test: {err}");
                Ok(None)
            }
        }
    }

    pub async fn start_anonymous() -> anyhow::Result<Option<Self>> {
        Self::start(&[]).await
    }

    pub async fn push(
        &self,
        repository: &str,
        tag: &str,
        auth: &RegistryAuth,
    ) -> anyhow::Result<TestImage> {
        let image = format!("{}/{repository}:{tag}", self.addr);
        let reference = Reference::try_from(image.as_str())?;

        let oci_client = Client::new(ClientConfig {
            protocol: ClientProtocol::Http,
            ..Default::default()
        });

        let layers = [ImageLayer::oci_v1(b"nirion-test-layer".to_vec(), None)];
        let config =
            Config::oci_v1_from_config_file(ConfigFile::default(), None)?;
        oci_client
            .push(&reference, &layers, config, auth, None)
            .await?;

        let (_, digest, _) = oci_client
            .pull_manifest_and_config(&reference, auth)
            .await?;

        Ok(TestImage {
            registry_addr: self.addr.clone(),
            reference,
            digest,
        })
    }

    pub async fn push_anonymous(
        &self,
        repository: &str,
        tag: &str,
    ) -> anyhow::Result<TestImage> {
        self.push(repository, tag, &RegistryAuth::Anonymous)
            .await
    }
}

pub async fn push_anonymous_test_image(
    tag: &str
) -> anyhow::Result<Option<(RegistryHandle, TestImage)>> {
    let Some(handle) = RegistryHandle::start_anonymous().await? else {
        return Ok(None);
    };

    let image = handle
        .push_anonymous("library/nirion-test", tag)
        .await?;
    Ok(Some((handle, image)))
}

pub fn http_nirion_client() -> crate::client::NirionOciClientBuilder {
    NirionOciClient::builder()
        .auth(AuthConfig::default())
        .oci_client_config(NirionOciClientConfig {
            protocol: ClientProtocol::Http,
            ..Default::default()
        })
}

fn registry_image() -> GenericImage {
    GenericImage::new("registry", "3")
        .with_exposed_port(5000.tcp())
        .with_wait_for(WaitFor::message_on_stderr("listening on"))
}
