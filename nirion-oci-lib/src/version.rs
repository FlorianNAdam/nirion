use semver::Version as SemverVersion;
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
pub struct VersionedImage {
    pub image: String,
    pub version: Option<String>,
    pub digest: String,
}

fn version_prefix(tag: &str) -> &str {
    tag.split('-').next().unwrap_or(tag)
}

fn suffix(tag: &str) -> Option<&str> {
    tag.split_once('-').map(|(_, s)| s)
}

fn parse_semver(tag: &str) -> Option<SemverVersion> {
    SemverVersion::parse(version_prefix(tag)).ok()
}

fn normalized_version_prefix(tag: &str) -> &str {
    let prefix = version_prefix(tag);
    prefix
        .strip_prefix('v')
        .unwrap_or(prefix)
}

fn version_depth(tag: &str) -> usize {
    normalized_version_prefix(tag)
        .split('.')
        .filter(|p| p.chars().all(|c| c.is_ascii_digit()))
        .count()
}

fn canonical_version_score(tag: &str) -> i32 {
    let floating = ["latest", "stable", "mainline"];
    if floating.contains(&tag) {
        return -1000;
    }

    let mut score = 0;

    if parse_semver(tag).is_some() {
        score += 50;
    }

    score += (version_depth(tag) as i32) * 10;

    if suffix(tag).is_some() {
        score += 5;
    }

    if suffix(tag)
        .map(|s| s.chars().any(|c| c.is_ascii_digit()))
        .unwrap_or(false)
    {
        score += 5;
    }

    score
}

pub fn canonical_version_tag(tags: &[String]) -> Option<String> {
    tags.iter()
        .max_by_key(|t| canonical_version_score(t))
        .cloned()
}
