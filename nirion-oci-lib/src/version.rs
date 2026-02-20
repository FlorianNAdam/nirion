use semver::Version as SemverVersion;
use serde::{Deserialize, Serialize};

pub const NON_VERSION_TAGS: &[&str] = &[
    // common floating tags / branch tags
    "latest",
    "stable",
    "nightly",
    "beta",
    "edge",
    "dev",
    "main",
    "master",
    "current",
    "next",
    "snapshot",
    "preview",
    "experimental",
    "production",
    "mainline",
    // architecture tags
    "amd64",
    "x86_64",
    "386",
    "i386",
    "arm64",
    "aarch64",
    "arm",
    "armv6",
    "armv7",
    "ppc64le",
    "s390x",
    "riscv64",
];

pub static TAG_PREFIXES: &[&str] = &[
    // git ref tags
    "refs/tags/version/",
    "refs/tags/",
];

pub static TAG_SUFFIXES: &[&str] = &[
    // Debian releases
    "-bookworm", // 12
    "-bullseye", // 11
    "-buster",   // 10
    "-stretch",  // 9
    "-jessie",   // 8
    "-wheezy",   // 7
    "-trixie",   // current testing
    "-sid",      // unstable
];

pub fn clean_tag(tag: &str) -> &str {
    let tag = tag.trim();

    let tag = strip_any_tag_prefix(tag, TAG_PREFIXES);
    strip_any_tag_suffix(tag, TAG_SUFFIXES)
}

pub fn strip_any_tag_prefix<'a>(s: &'a str, prefixes: &[&str]) -> &'a str {
    prefixes
        .iter()
        .find_map(|p| {
            s.strip_prefix(p)
                .filter(|t| !t.is_empty())
        })
        .unwrap_or(s)
}

pub fn strip_any_tag_suffix<'a>(s: &'a str, suffixes: &[&str]) -> &'a str {
    suffixes
        .iter()
        .find_map(|p| {
            s.strip_suffix(p)
                .filter(|t| !t.is_empty())
        })
        .unwrap_or(s)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
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

pub fn canonical_version_score(tag: &str) -> i32 {
    if NON_VERSION_TAGS.contains(&tag) {
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
        .filter(|version| !NON_VERSION_TAGS.contains(&version.as_str()))
        .map(|t| clean_tag(t))
        .max_by_key(|t| canonical_version_score(t))
        .map(|t| t.to_string())
}
