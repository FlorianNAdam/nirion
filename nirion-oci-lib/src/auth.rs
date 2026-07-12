use oci_client::secrets::RegistryAuth as OciRegistryAuth;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Hash)]
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
            Basic(username, password) => Self::basic(username, password),
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

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{AUTHORIZATION, HeaderValue};

    #[test]
    fn constructors_create_expected_auth_variants() {
        assert!(matches!(RegistryAuth::anonymous(), RegistryAuth::Anonymous));

        match RegistryAuth::basic("user", "pass") {
            RegistryAuth::Basic { username, password } => {
                assert_eq!(username, "user");
                assert_eq!(password, "pass");
            }
            other => panic!("unexpected auth variant: {other:?}"),
        }

        match RegistryAuth::bearer("token") {
            RegistryAuth::Bearer { token } => assert_eq!(token, "token"),
            other => panic!("unexpected auth variant: {other:?}"),
        }
    }

    #[test]
    fn converts_to_oci_auth() {
        assert_eq!(
            RegistryAuth::anonymous().to_oci_auth(),
            OciRegistryAuth::Anonymous
        );
        assert_eq!(
            RegistryAuth::basic("user", "pass").to_oci_auth(),
            OciRegistryAuth::Basic("user".to_string(), "pass".to_string())
        );
        assert_eq!(
            RegistryAuth::bearer("token").to_oci_auth(),
            OciRegistryAuth::Bearer("token".to_string())
        );
    }

    #[test]
    fn converts_from_oci_auth() {
        assert!(matches!(
            RegistryAuth::from_oci_auth(&OciRegistryAuth::Anonymous),
            RegistryAuth::Anonymous
        ));

        match RegistryAuth::from_oci_auth(&OciRegistryAuth::Basic(
            "user".to_string(),
            "pass".to_string(),
        )) {
            RegistryAuth::Basic { username, password } => {
                assert_eq!(username, "user");
                assert_eq!(password, "pass");
            }
            other => panic!("unexpected auth variant: {other:?}"),
        }

        match RegistryAuth::from_oci_auth(&OciRegistryAuth::Bearer(
            "token".to_string(),
        )) {
            RegistryAuth::Bearer { token } => assert_eq!(token, "token"),
            other => panic!("unexpected auth variant: {other:?}"),
        }
    }

    #[test]
    fn applies_basic_authentication_to_request() {
        let request = reqwest::Client::new()
            .get("http://example.test")
            .apply_authentication(&RegistryAuth::basic("user", "pass"))
            .build()
            .unwrap();

        assert_eq!(
            request.headers().get(AUTHORIZATION),
            Some(&HeaderValue::from_static("Basic dXNlcjpwYXNz"))
        );
    }

    #[test]
    fn applies_bearer_authentication_to_request() {
        let request = reqwest::Client::new()
            .get("http://example.test")
            .apply_authentication(&RegistryAuth::bearer("token"))
            .build()
            .unwrap();

        assert_eq!(
            request.headers().get(AUTHORIZATION),
            Some(&HeaderValue::from_static("Bearer token"))
        );
    }

    #[test]
    fn anonymous_authentication_leaves_request_unchanged() {
        let request = reqwest::Client::new()
            .get("http://example.test")
            .apply_authentication(&RegistryAuth::anonymous())
            .build()
            .unwrap();

        assert!(
            !request
                .headers()
                .contains_key(AUTHORIZATION)
        );
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
