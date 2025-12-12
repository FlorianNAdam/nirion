use tokio::process::Command;

pub async fn fetch_digest(image: &str) -> anyhow::Result<String> {
    let output = Command::new("skopeo")
        .args([
            "inspect",
            "--format",
            "{{.Digest}}",
            &format!("docker://{}", image),
        ])
        .output()
        .await?;

    if !output.status.success() {
        anyhow::bail!("Failed to fetch digest")
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_string())
}
