use std::collections::HashMap;

use nirion_oci_lib::{
    auth::RegistryAuth, oci::resolve_registry, oci_client::Client,
};
use serde::Deserialize;

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
            .map(|(registry, auth)| (resolve_registry(registry), auth))
            .collect();

        Ok(AuthConfig {
            sources: resolved_sources,
        })
    }
}

impl AuthConfig {
    pub async fn get_oci_client(&self) -> Client {
        let client = Client::default();

        for (registry, auth) in self.sources.iter() {
            client
                .store_auth_if_needed(&registry, &auth.to_oci_auth())
                .await;
        }

        client
    }

    pub fn add_auth(&mut self, registry: String, auth: RegistryAuth) {
        self.sources.insert(registry, auth);
    }
}
