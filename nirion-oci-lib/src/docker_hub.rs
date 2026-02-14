use oci_client::{config::Architecture, Reference};
use reqwest::StatusCode;
use serde::Deserialize;
use thiserror::Error;

const DOCKERHUB_BASE: &str = "https://hub.docker.com/v2";

#[derive(Debug, Error)]
pub enum DockerHubError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("DockerHub API error: {detail:?} {message:?}")]
    Api {
        detail: Option<String>,
        message: Option<String>,
    },

    #[error("Unexpected status code: {0}")]
    UnexpectedStatus(StatusCode),

    #[error("Only docker.io images are supported")]
    UnsupportedRegistry,

    #[error("Image reference must include an explicit tag")]
    MissingTag,

    #[error("Digest references are not supported for this endpoint")]
    DigestNotSupported,

    #[error("Tag not found")]
    TagNotFound,

    #[error("Image not found")]
    ImageNotFound,

    #[error("Missing Digest")]
    MissingDigest,
}

#[derive(Debug, Deserialize)]
pub struct TagsResponse {
    pub count: u64,
    pub next: Option<String>,
    pub previous: Option<String>,
    pub results: Vec<Tag>,
}

#[derive(Debug, Deserialize)]
pub struct Tag {
    pub id: u64,
    pub images: Vec<Image>,
    pub creator: u64,
    pub last_updated: Option<String>,
    pub last_updater: u64,
    pub last_updater_username: String,
    pub name: String,
    pub repository: u64,
    pub full_size: u64,
    pub v2: bool,
    pub status: Option<String>,
    pub tag_last_pulled: Option<String>,
    pub tag_last_pushed: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Image {
    pub architecture: Architecture,
    pub features: String,
    pub variant: Option<String>,
    pub digest: Option<String>,
    pub layers: Option<Vec<Layer>>,
    pub os: String,
    pub os_features: String,
    pub os_version: Option<String>,
    pub size: u64,
    pub status: String,
    pub last_pulled: Option<String>,
    pub last_pushed: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Layer {
    pub digest: Option<String>,
    pub size: u64,
    pub instruction: String,
}

#[derive(Debug, Deserialize)]
pub struct ApiErrorResponse {
    pub detail: Option<String>,
    pub message: Option<String>,
}

fn dockerhub_parts(
    reference: &Reference,
) -> Result<(String, String), DockerHubError> {
    let registry = reference.registry();

    if registry != "docker.io" {
        return Err(DockerHubError::UnsupportedRegistry);
    }

    let repo = reference.repository();

    let mut parts = repo.split('/');

    let first = parts.next().unwrap();
    let second = parts.next();

    match second {
        Some(rest) => Ok((first.to_string(), format!("{rest}"))),
        None => Ok(("library".to_string(), first.to_string())),
    }
}

pub async fn fetch_dockerhub_tags_page(
    reference: &Reference,
    page_size: u32,
    page: u32,
) -> Result<TagsResponse, DockerHubError> {
    let client = reqwest::Client::new();
    let (namespace, repository) = dockerhub_parts(reference)?;

    let url = format!(
        "{base}/repositories/{namespace}/{repository}/tags?page_size={page_size}&page={page}",
        base = DOCKERHUB_BASE
    );

    let resp = client.get(&url).send().await?;

    if resp.status().is_success() {
        Ok(resp.json::<TagsResponse>().await?)
    } else {
        let status = resp.status();
        if let Ok(api_err) = resp.json::<ApiErrorResponse>().await {
            Err(DockerHubError::Api {
                detail: api_err.detail,
                message: api_err.message,
            })
        } else {
            Err(DockerHubError::UnexpectedStatus(status))
        }
    }
}

pub async fn fetch_all_dockerhub_tags(
    reference: &Reference,
    page_size: u32,
) -> Result<TagsResponse, DockerHubError> {
    let client = reqwest::Client::new();
    let (namespace, repository) = dockerhub_parts(reference)?;

    let mut next_url = Some(format!(
        "{base}/repositories/{namespace}/{repository}/tags?page_size={page_size}&page=1",
        base = DOCKERHUB_BASE
    ));

    let mut all_results = Vec::new();
    let mut total_count = 0;

    while let Some(url) = next_url {
        let resp = client.get(&url).send().await?;

        if resp.status().is_success() {
            let mut body: TagsResponse = resp.json().await?;
            total_count = body.count;
            all_results.append(&mut body.results);
            next_url = body.next;
        } else {
            let status = resp.status();
            if let Ok(api_err) = resp.json::<ApiErrorResponse>().await {
                return Err(DockerHubError::Api {
                    detail: api_err.detail,
                    message: api_err.message,
                });
            } else {
                return Err(DockerHubError::UnexpectedStatus(status));
            }
        }
    }

    Ok(TagsResponse {
        count: total_count,
        next: None,
        previous: None,
        results: all_results,
    })
}

pub async fn fetch_dockerhub_tag(
    reference: &Reference,
) -> Result<Tag, DockerHubError> {
    let client = reqwest::Client::new();
    let (namespace, repository) = dockerhub_parts(reference)?;

    if reference.digest().is_some() {
        return Err(DockerHubError::DigestNotSupported);
    }

    let tag = reference
        .tag()
        .ok_or(DockerHubError::MissingTag)?;

    let url = format!(
        "{base}/namespaces/{namespace}/repositories/{repository}/tags/{tag}",
        base = DOCKERHUB_BASE
    );

    let resp = client.get(&url).send().await?;

    if resp.status().is_success() {
        Ok(resp.json::<Tag>().await?)
    } else {
        let status = resp.status();
        if let Ok(api_err) = resp.json::<ApiErrorResponse>().await {
            Err(DockerHubError::Api {
                detail: api_err.detail,
                message: api_err.message,
            })
        } else {
            Err(DockerHubError::UnexpectedStatus(status))
        }
    }
}

pub async fn get_alias_dockerhub_tags(
    image: &Reference,
    digest: &str,
) -> anyhow::Result<Vec<String>> {
    let arch = Architecture::default();

    let mut tags = fetch_all_dockerhub_tags(&image, 100)
        .await?
        .results;

    for tag in tags.iter_mut() {
        tag.images
            .retain(|image| image.architecture == arch);
    }

    let other_tags = tags
        .iter()
        .filter(|t| {
            t.images.iter().any(|i| {
                i.digest
                    .as_ref()
                    .is_some_and(|d| d == digest)
            })
        })
        .collect::<Vec<_>>();

    let candidates = other_tags
        .into_iter()
        .map(|tag| tag.name.clone())
        .collect::<Vec<_>>();

    Ok(candidates)
}
