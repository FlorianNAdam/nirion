use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::Deserialize;

use crate::{
    auth::RegistryAuth,
    docker_hub::DockerHubClient,
    get_updated_versioned_image_with_auth, get_versioned_image_with_auth,
    oci::resolve_registry,
    oci_client::{
        Client, Reference,
        client::{Certificate, ClientConfig, ClientProtocol},
    },
    version::VersionedImage,
};

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
    fn to_oci_client_config(&self) -> ClientConfig {
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
    pub fn add_auth(&mut self, registry: String, auth: RegistryAuth) {
        self.sources
            .insert(normalize_scope(&registry), auth);
    }

    pub fn auth_for(&self, image: &Reference) -> RegistryAuth {
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ClientKey {
    registry: String,
    auth: RegistryAuth,
}

pub struct NirionOciClient {
    auth: AuthConfig,
    docker_hub: DockerHubClient,
    oci_client_config: NirionOciClientConfig,
    clients: Mutex<HashMap<ClientKey, Arc<Client>>>,
}

impl NirionOciClient {
    pub fn builder() -> NirionOciClientBuilder {
        NirionOciClientBuilder::default()
    }

    pub async fn get_versioned_image(
        &self,
        image: &Reference,
    ) -> anyhow::Result<VersionedImage> {
        let auth = self.auth.auth_for(image);
        let client = self.client_for(image, &auth).await;
        let oci_auth = auth.to_oci_auth();

        get_versioned_image_with_auth(
            &client,
            &self.docker_hub,
            image,
            &oci_auth,
        )
        .await
    }

    pub async fn get_updated_versioned_image(
        &self,
        versioned_image: &VersionedImage,
    ) -> anyhow::Result<VersionedImage> {
        let image = Reference::try_from(versioned_image.image.as_str())?;
        let auth = self.auth.auth_for(&image);
        let client = self.client_for(&image, &auth).await;
        let oci_auth = auth.to_oci_auth();

        get_updated_versioned_image_with_auth(
            &client,
            &self.docker_hub,
            versioned_image,
            &oci_auth,
        )
        .await
    }

    async fn client_for(
        &self,
        image: &Reference,
        auth: &RegistryAuth,
    ) -> Arc<Client> {
        let key = ClientKey {
            registry: image.resolve_registry().to_string(),
            auth: auth.clone(),
        };

        if let Some(client) = self
            .clients
            .lock()
            .unwrap()
            .get(&key)
            .cloned()
        {
            return client;
        }

        let client = Arc::new(Client::new(
            self.oci_client_config
                .to_oci_client_config(),
        ));
        client
            .store_auth_if_needed(&key.registry, &auth.to_oci_auth())
            .await;

        self.clients
            .lock()
            .unwrap()
            .entry(key)
            .or_insert_with(|| Arc::clone(&client))
            .clone()
    }
}

pub struct NirionOciClientBuilder {
    auth: AuthConfig,
    docker_hub: DockerHubClient,
    oci_client_config: NirionOciClientConfig,
}

impl Default for NirionOciClientBuilder {
    fn default() -> Self {
        Self {
            auth: AuthConfig::default(),
            docker_hub: DockerHubClient::default(),
            oci_client_config: NirionOciClientConfig::default(),
        }
    }
}

impl NirionOciClientBuilder {
    pub fn auth(mut self, auth: AuthConfig) -> Self {
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

    pub fn docker_hub(mut self, docker_hub: DockerHubClient) -> Self {
        self.docker_hub = docker_hub;
        self
    }

    pub fn oci_client_config(mut self, config: NirionOciClientConfig) -> Self {
        self.oci_client_config = config;
        self
    }

    pub fn oci_client_protocol(mut self, protocol: ClientProtocol) -> Self {
        self.oci_client_config.protocol = protocol;
        self
    }

    pub fn build(self) -> NirionOciClient {
        NirionOciClient {
            auth: self.auth,
            docker_hub: self.docker_hub,
            oci_client_config: self.oci_client_config,
            clients: Mutex::new(HashMap::new()),
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
}
