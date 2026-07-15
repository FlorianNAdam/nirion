use std::{
    fs,
    net::TcpListener,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    client::{AuthConfig, NirionOciClient, NirionOciClientConfig},
    oci_client::{
        Client, Reference,
        client::{ClientConfig, ClientProtocol, Config, ImageLayer},
        config::ConfigFile,
        secrets::RegistryAuth,
    },
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
    child: Child,
    _temp_dir: TempDir,
    pub addr: String,
}

impl RegistryHandle {
    pub async fn start(accounts: &[TestAccount]) -> anyhow::Result<Self> {
        let temp_dir = TempDir::new()?;
        let port = unused_local_port()?;
        let addr = format!("127.0.0.1:{port}");
        let config = write_registry_config(temp_dir.path(), &addr, accounts)?;
        let command = std::env::var("NIRION_TEST_REGISTRY_COMMAND")
            .unwrap_or_else(|_| "registry".to_string());

        let child = Command::new(&command)
            .arg("serve")
            .arg(&config)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| {
                anyhow::anyhow!(
                    "failed to start test OCI registry using `{command}`; run through nix flake check or set NIRION_TEST_REGISTRY_COMMAND: {err}"
                )
            })?;

        let mut handle = Self {
            child,
            _temp_dir: temp_dir,
            addr,
        };
        if let Err(err) = handle.wait_until_ready().await {
            let _ = handle.child.kill();
            let _ = handle.child.wait();
            return Err(err);
        }

        Ok(handle)
    }

    pub async fn start_anonymous() -> anyhow::Result<Self> {
        Self::start(&[]).await
    }

    pub async fn push(
        &self,
        repository: &str,
        tag: &str,
        auth: &RegistryAuth,
    ) -> anyhow::Result<TestImage> {
        let image = format!("/{repository}:{tag}");
        let image = format!("{}{image}", self.addr);
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

    async fn wait_until_ready(&mut self) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let url = format!("http://{}/v2/", self.addr);

        for _ in 0..100 {
            if let Some(status) = self.child.try_wait()? {
                anyhow::bail!(
                    "test OCI registry exited before becoming ready: {status}"
                );
            }

            match client.get(&url).send().await {
                Ok(response)
                    if response.status().is_success()
                        || response.status().as_u16() == 401 =>
                {
                    return Ok(());
                }
                _ => {
                    tokio::time::sleep(std::time::Duration::from_millis(50))
                        .await
                }
            }
        }

        anyhow::bail!("test OCI registry did not become ready at {url}")
    }
}

impl Drop for RegistryHandle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub async fn push_anonymous_test_image(
    tag: &str
) -> anyhow::Result<(RegistryHandle, TestImage)> {
    let handle = RegistryHandle::start_anonymous().await?;
    let image = handle
        .push_anonymous("library/nirion-test", tag)
        .await?;
    Ok((handle, image))
}

pub fn http_nirion_client() -> crate::client::NirionOciClientBuilder {
    NirionOciClient::builder()
        .auth(AuthConfig::default())
        .oci_client_config(NirionOciClientConfig {
            protocol: ClientProtocol::Http,
            ..Default::default()
        })
}

fn write_registry_config(
    dir: &std::path::Path,
    addr: &str,
    accounts: &[TestAccount],
) -> anyhow::Result<PathBuf> {
    let storage = dir.join("storage");
    fs::create_dir(&storage)?;

    let auth = if accounts.is_empty() {
        String::new()
    } else {
        let htpasswd = dir.join("htpasswd");
        fs::write(
            &htpasswd,
            accounts
                .iter()
                .map(|account| account.htpasswd_line)
                .collect::<Vec<_>>()
                .join("\n"),
        )?;
        format!(
            r#"
auth:
  htpasswd:
    realm: Registry Realm
    path: {}
"#,
            htpasswd.display()
        )
    };

    let config = dir.join("config.yml");
    fs::write(
        &config,
        format!(
            r#"version: 0.1
log:
  level: error
storage:
  filesystem:
    rootdirectory: {}
http:
  addr: {}
{}
"#,
            storage.display(),
            addr,
            auth
        ),
    )?;

    Ok(config)
}

fn unused_local_port() -> anyhow::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> anyhow::Result<Self> {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_nanos();
        let path = std::env::temp_dir()
            .join(format!("nirion-test-registry-{}-{id}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
