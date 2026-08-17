use std::{collections::HashMap, time::Duration};

use oci_client::config::ConfigFile;
use serde::Deserialize;

use crate::{
    auth::RegistryAuth,
    docker_hub::DockerHubClient,
    oci::{RegistryClient, get_version_from_config, resolve_registry},
    oci_client::{
        Reference,
        client::{Certificate, ClientConfig, ClientProtocol},
    },
    version::VersionedImage,
};

#[derive(Clone, Debug)]
pub struct VersionedImageResolution {
    pub image: VersionedImage,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct NirionOciClientConfig {
    pub protocol: ClientProtocol,
    pub accept_invalid_certificates: bool,
    pub use_monolithic_push: bool,
    pub tls_certs_only: Vec<Certificate>,
    pub extra_root_certificates: Vec<Certificate>,
    pub max_concurrent_upload: usize,
    pub max_concurrent_download: usize,
    pub default_token_expiration_secs: usize,
    pub read_timeout: Option<Duration>,
    pub connect_timeout: Option<Duration>,
    pub user_agent: &'static str,
    pub https_proxy: Option<String>,
    pub http_proxy: Option<String>,
    pub no_proxy: Option<String>,
}

impl Default for NirionOciClientConfig {
    fn default() -> Self {
        let config = ClientConfig::default();
        Self {
            protocol: config.protocol,
            accept_invalid_certificates: config.accept_invalid_certificates,
            use_monolithic_push: config.use_monolithic_push,
            tls_certs_only: config.tls_certs_only,
            extra_root_certificates: config.extra_root_certificates,
            max_concurrent_upload: config.max_concurrent_upload,
            max_concurrent_download: config.max_concurrent_download,
            default_token_expiration_secs: config.default_token_expiration_secs,
            read_timeout: config.read_timeout,
            connect_timeout: config.connect_timeout,
            user_agent: config.user_agent,
            https_proxy: config.https_proxy,
            http_proxy: config.http_proxy,
            no_proxy: config.no_proxy,
        }
    }
}

impl NirionOciClientConfig {
    pub(crate) fn to_oci_client_config(&self) -> ClientConfig {
        ClientConfig {
            protocol: self.protocol.clone(),
            accept_invalid_certificates: self.accept_invalid_certificates,
            use_monolithic_push: self.use_monolithic_push,
            tls_certs_only: self.tls_certs_only.clone(),
            extra_root_certificates: self.extra_root_certificates.clone(),
            max_concurrent_upload: self.max_concurrent_upload,
            max_concurrent_download: self.max_concurrent_download,
            default_token_expiration_secs: self.default_token_expiration_secs,
            read_timeout: self.read_timeout,
            connect_timeout: self.connect_timeout,
            user_agent: self.user_agent,
            https_proxy: self.https_proxy.clone(),
            http_proxy: self.http_proxy.clone(),
            no_proxy: self.no_proxy.clone(),
            ..Default::default()
        }
    }
}

#[derive(Default, Clone, Debug)]
pub struct AuthConfig {
    pub sources: HashMap<String, RegistryAuth>,
}

impl<'de> Deserialize<'de> for AuthConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let sources =
            HashMap::<String, RegistryAuth>::deserialize(deserializer)?;

        let resolved_sources = sources
            .into_iter()
            .map(|(scope, auth)| (normalize_scope(&scope), auth))
            .collect();

        Ok(AuthConfig {
            sources: resolved_sources,
        })
    }
}

impl AuthConfig {
    pub fn add_auth(
        &mut self,
        registry: String,
        auth: RegistryAuth,
    ) {
        self.sources
            .insert(normalize_scope(&registry), auth);
    }

    pub fn auth_for(
        &self,
        image: &Reference,
    ) -> RegistryAuth {
        let registry = resolve_registry(image.registry().to_string());
        let mut key = format!("{}/{}", registry, image.repository());

        loop {
            if let Some(auth) = self.sources.get(&key) {
                return auth.clone();
            }

            if let Some((parent, _)) = key.rsplit_once('/') {
                key = parent.to_string();
            } else {
                break;
            }
        }

        self.sources
            .get(&registry)
            .cloned()
            .unwrap_or_else(RegistryAuth::anonymous)
    }
}

fn normalize_scope(scope: &str) -> String {
    let mut parts = scope.splitn(2, '/');
    let registry = parts.next().unwrap_or_default();
    let registry = resolve_registry(registry.to_string());

    if let Some(repository) = parts.next() {
        format!("{registry}/{repository}")
    } else {
        registry
    }
}

pub struct NirionOciClient {
    auth: AuthConfig,
    docker_hub_client: DockerHubClient,
    registry_client: RegistryClient,
}

impl NirionOciClient {
    pub fn builder() -> NirionOciClientBuilder {
        NirionOciClientBuilder::default()
    }

    pub async fn get_versioned_image(
        &self,
        image: &Reference,
    ) -> anyhow::Result<VersionedImage> {
        Ok(self
            .get_versioned_image_resolution(image)
            .await?
            .image)
    }

    pub async fn get_versioned_image_resolution(
        &self,
        image: &Reference,
    ) -> anyhow::Result<VersionedImageResolution> {
        let auth = self.auth.auth_for(image);

        let (version, digest, warnings) = self
            .resolve_version_and_digest(image, auth)
            .await?;

        Ok(VersionedImageResolution {
            image: VersionedImage {
                image: image.to_string(),
                version,
                digest,
            },
            warnings,
        })
    }

    pub async fn get_updated_versioned_image(
        &self,
        versioned_image: &VersionedImage,
    ) -> anyhow::Result<VersionedImage> {
        Ok(self
            .get_updated_versioned_image_resolution(versioned_image)
            .await?
            .image)
    }

    pub async fn get_updated_versioned_image_resolution(
        &self,
        versioned_image: &VersionedImage,
    ) -> anyhow::Result<VersionedImageResolution> {
        let image = Reference::try_from(versioned_image.image.as_str())?;
        let auth = self.auth.auth_for(&image);

        let (_, current_digest, _) = self
            .registry_client
            .pull_manifest_and_config(&image, auth.clone())
            .await?;

        if current_digest == versioned_image.digest {
            return Ok(VersionedImageResolution {
                image: VersionedImage {
                    image: versioned_image.image.clone(),
                    version: versioned_image.version.clone(),
                    digest: versioned_image.digest.clone(),
                },
                warnings: Vec::new(),
            });
        }

        let (version, digest, warnings) = self
            .resolve_version_and_digest(&image, auth)
            .await?;

        Ok(VersionedImageResolution {
            image: VersionedImage {
                image: versioned_image.image.clone(),
                version,
                digest,
            },
            warnings,
        })
    }

    async fn resolve_version_and_digest(
        &self,
        image: &Reference,
        auth: RegistryAuth,
    ) -> anyhow::Result<(Option<String>, String, Vec<String>)> {
        let (_, digest, raw_config) = self
            .registry_client
            .pull_manifest_and_config(image, auth.clone())
            .await?;

        let config: ConfigFile = serde_json::from_str(&raw_config)?;

        if let Some(version) = get_version_from_config(&config) {
            return Ok((Some(version), digest, Vec::new()));
        }

        let (version, warnings) = match self
            .resolve_version_from_tags(image, &digest, auth)
            .await
        {
            Ok(version) => (version, Vec::new()),
            Err(error) => (
                None,
                vec![format!(
                    "Failed to resolve version tag for {image}: {error}"
                )],
            ),
        };

        Ok((version, digest, warnings))
    }

    async fn resolve_version_from_tags(
        &self,
        image: &Reference,
        digest: &str,
        auth: RegistryAuth,
    ) -> anyhow::Result<Option<String>> {
        if self.docker_hub_client.supports(image) {
            self.docker_hub_client
                .version_from_tags(image, digest, auth)
                .await
        } else {
            self.registry_client
                .version_from_tags(image, digest, auth)
                .await
        }
    }
}

pub struct NirionOciClientBuilder {
    auth: AuthConfig,
    docker_hub_client: DockerHubClient,
    registry_client: RegistryClient,
}

impl Default for NirionOciClientBuilder {
    fn default() -> Self {
        Self {
            auth: AuthConfig::default(),
            docker_hub_client: DockerHubClient::default(),
            registry_client: RegistryClient::default(),
        }
    }
}

impl NirionOciClientBuilder {
    pub fn auth(
        mut self,
        auth: AuthConfig,
    ) -> Self {
        self.auth = auth;
        self
    }

    pub fn add_auth(
        mut self,
        scope: impl Into<String>,
        auth: RegistryAuth,
    ) -> Self {
        self.auth.add_auth(scope.into(), auth);
        self
    }

    pub fn docker_hub_client(
        mut self,
        docker_hub_client: DockerHubClient,
    ) -> Self {
        self.docker_hub_client = docker_hub_client;
        self
    }

    pub fn registry_client(
        mut self,
        registry_client: RegistryClient,
    ) -> Self {
        self.registry_client = registry_client;
        self
    }

    pub fn build(self) -> NirionOciClient {
        NirionOciClient {
            auth: self.auth,
            docker_hub_client: self.docker_hub_client,
            registry_client: self.registry_client,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth(username: &str) -> RegistryAuth {
        RegistryAuth::basic(username, "password")
    }

    fn username(auth: RegistryAuth) -> Option<String> {
        match auth {
            RegistryAuth::Basic { username, .. } => Some(username),
            _ => None,
        }
    }

    #[test]
    fn auth_for_uses_registry_auth() {
        let mut config = AuthConfig::default();
        config.add_auth("docker.io".to_string(), auth("registry"));

        let image =
            Reference::try_from("docker.io/library/nginx:latest").unwrap();

        assert_eq!(
            username(config.auth_for(&image)),
            Some("registry".to_string())
        );
    }

    #[test]
    fn auth_for_uses_longest_repository_prefix() {
        let mut config = AuthConfig::default();
        config.add_auth("docker.io".to_string(), auth("registry"));
        config.add_auth("docker.io/org-a".to_string(), auth("org"));
        config.add_auth("docker.io/org-a/app".to_string(), auth("app"));

        let app = Reference::try_from("docker.io/org-a/app:latest").unwrap();
        let other =
            Reference::try_from("docker.io/org-a/other:latest").unwrap();
        let fallback =
            Reference::try_from("docker.io/org-b/app:latest").unwrap();

        assert_eq!(username(config.auth_for(&app)), Some("app".to_string()));
        assert_eq!(username(config.auth_for(&other)), Some("org".to_string()));
        assert_eq!(
            username(config.auth_for(&fallback)),
            Some("registry".to_string())
        );
    }

    #[test]
    fn auth_for_defaults_to_anonymous() {
        let config = AuthConfig::default();
        let image = Reference::try_from("ghcr.io/example/app:latest").unwrap();

        assert!(matches!(config.auth_for(&image), RegistryAuth::Anonymous));
    }

    #[test]
    fn deserialization_normalizes_auth_scopes() {
        let config: AuthConfig = serde_json::from_str(
            r#"{
                "docker.io/library": {
                    "type": "basic",
                    "username": "user",
                    "password": "password"
                }
            }"#,
        )
        .unwrap();

        let image = Reference::try_from("index.docker.io/library/nginx:latest")
            .unwrap();

        assert_eq!(username(config.auth_for(&image)), Some("user".to_string()));
    }

    #[test]
    fn builder_add_auth_and_protocol_configures_client() {
        let client = NirionOciClient::builder()
            .add_auth("ghcr.io/example", auth("repo"))
            .registry_client(RegistryClient::new(NirionOciClientConfig {
                protocol: ClientProtocol::Http,
                ..Default::default()
            }))
            .build();

        let image = Reference::try_from("ghcr.io/example/app:latest").unwrap();

        assert_eq!(
            username(client.auth.auth_for(&image)),
            Some("repo".to_string())
        );
        assert_eq!(
            client.registry_client.config().protocol,
            ClientProtocol::Http
        );
    }

    #[test]
    fn builder_auth_docker_hub_and_config_configure_client() {
        let mut auth_config = AuthConfig::default();
        auth_config.add_auth("ghcr.io".to_string(), auth("registry"));
        let docker_hub = DockerHubClient::default()
            .with_registries(["localhost:5000".to_string()]);
        let config = NirionOciClientConfig {
            protocol: ClientProtocol::Http,
            ..NirionOciClientConfig::default()
        };

        let client = NirionOciClient::builder()
            .auth(auth_config)
            .docker_hub_client(docker_hub)
            .registry_client(RegistryClient::new(config))
            .build();

        let ghcr = Reference::try_from("ghcr.io/example/app:latest").unwrap();
        let local = Reference::try_from("localhost:5000/nginx:latest").unwrap();

        assert_eq!(
            username(client.auth.auth_for(&ghcr)),
            Some("registry".to_string())
        );
        assert!(
            client
                .docker_hub_client
                .supports(&local)
        );
        assert_eq!(
            client.registry_client.config().protocol,
            ClientProtocol::Http
        );
    }
}
