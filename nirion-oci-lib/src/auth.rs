use oci_client::secrets::RegistryAuth as OciRegistryAuth;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RegistryAuth {
    Anonymous,
    Basic { username: String, password: String },
    Bearer { token: String },
}

impl RegistryAuth {
    pub fn anonymous() -> Self {
        Self::Anonymous
    }

    pub fn basic(
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self::Basic {
            username: username.into(),
            password: password.into(),
        }
    }

    pub fn bearer(token: impl Into<String>) -> Self {
        Self::Bearer {
            token: token.into(),
        }
    }

    pub fn from_oci_auth(auth: &OciRegistryAuth) -> Self {
        use OciRegistryAuth::*;
        match auth {
            Anonymous => Self::anonymous(),
            Basic(password, username) => Self::basic(username, password),
            Bearer(token) => Self::bearer(token),
        }
    }

    pub fn to_oci_auth(&self) -> OciRegistryAuth {
        match self {
            Self::Anonymous => OciRegistryAuth::Anonymous,
            Self::Basic { username, password } => OciRegistryAuth::Basic(
                username.to_string(),
                password.to_string(),
            ),
            Self::Bearer { token } => {
                OciRegistryAuth::Bearer(token.to_string())
            }
        }
    }
}

pub trait Authenticable {
    fn apply_authentication(self, auth: &RegistryAuth) -> Self;
}

impl Authenticable for reqwest::RequestBuilder {
    fn apply_authentication(self, auth: &RegistryAuth) -> Self {
        match auth {
            RegistryAuth::Anonymous => self,
            RegistryAuth::Basic { username, password } => {
                self.basic_auth(username, Some(password))
            }
            RegistryAuth::Bearer { token } => self.bearer_auth(token),
        }
    }
}
