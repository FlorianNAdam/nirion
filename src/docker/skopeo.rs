use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;
use tokio::process::Command;

#[allow(unused)]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ImageInspect {
    pub name: String,
    pub digest: String,
    pub repo_tags: Vec<String>,
    pub created: String,
    pub docker_version: String,
    pub labels: Option<HashMap<String, String>>,
    pub architecture: String,
    pub os: String,
    pub layers: Vec<String>,
    pub layers_data: Option<Vec<LayerData>>,
    pub env: Option<Vec<String>>,
}

#[allow(unused)]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LayerData {
    #[serde(rename = "MIMEType")]
    pub mime_type: String,
    pub digest: String,
    pub size: usize,
    pub annotations: Value,
}

pub async fn inspect(image: &str) -> anyhow::Result<ImageInspect> {
    let output = Command::new("skopeo")
        .args(["inspect", &format!("docker://{}", image)])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to fetch digest for {}:\n{}", image, stderr)
    }

    let json = str::from_utf8(&output.stdout)?
        .trim()
        .to_string();

    let inspect: ImageInspect = serde_json::from_str(&json)?;

    Ok(inspect)
}
