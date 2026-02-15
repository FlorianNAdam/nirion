use std::collections::HashMap;

use nirion_oci_lib::{
    oci::resolve_registry,
    oci_client::{secrets::RegistryAuth, Client},
};
use serde::Deserialize;

#[derive(Default, Clone, Debug)]
pub struct AuthConfig {
    pub sources: HashMap<String, RegistryAuthConfig>,
}

impl<'de> Deserialize<'de> for AuthConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let sources =
            HashMap::<String, RegistryAuthConfig>::deserialize(deserializer)?;

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
    pub async fn get_client(&self) -> Client {
        let client = Client::default();

        for (registry, auth) in self.sources.iter() {
            client
                .store_auth_if_needed(&registry, &auth.to_auth())
                .await;
        }

        client
    }

    pub fn add_auth(&mut self, registry: String, auth: RegistryAuthConfig) {
        self.sources.insert(registry, auth);
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RegistryAuthConfig {
    Anonymous,
    Basic { username: String, password: String },
    Bearer { token: String },
}

impl RegistryAuthConfig {
    pub fn to_auth(&self) -> RegistryAuth {
        match self {
            Self::Anonymous => RegistryAuth::Anonymous,
            Self::Basic { username, password } => {
                RegistryAuth::Basic(username.to_string(), password.to_string())
            }
            Self::Bearer { token } => RegistryAuth::Bearer(token.to_string()),
        }
    }
}
