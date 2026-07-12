use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use serde::Deserialize;

use crate::{
    auth::RegistryAuth,
    docker_hub::DockerHubClient,
    get_updated_versioned_image_with_auth, get_versioned_image_with_auth,
    oci::resolve_registry,
    oci_client::{Client, Reference},
    version::VersionedImage,
};

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
    clients: Mutex<HashMap<ClientKey, Arc<Client>>>,
}

impl NirionOciClient {
    pub fn new(auth: AuthConfig) -> Self {
        Self {
            auth,
            docker_hub: DockerHubClient::default(),
            clients: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_docker_hub(mut self, docker_hub: DockerHubClient) -> Self {
        self.docker_hub = docker_hub;
        self
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

        let client = Arc::new(Client::default());
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
