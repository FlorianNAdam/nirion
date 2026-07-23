use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub use nirion_oci_lib::version::VersionedImage;

#[derive(Default, Clone, PartialEq)]
pub struct LockedImages {
    locked_images: BTreeMap<String, VersionedImage>,
}

impl<'de> Deserialize<'de> for LockedImages {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let locked_images =
            BTreeMap::<String, VersionedImage>::deserialize(deserializer)?;

        Ok(Self { locked_images })
    }
}

impl Serialize for LockedImages {
    fn serialize<S>(
        &self,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.locked_images.serialize(serializer)
    }
}

impl LockedImages {
    pub fn iter(&self) -> impl Iterator<Item = (&str, &VersionedImage)> {
        self.locked_images
            .iter()
            .map(|(k, v)| (k.as_str(), v))
    }

    pub fn insert(
        &mut self,
        key: String,
        image: VersionedImage,
    ) {
        self.locked_images.insert(key, image);
    }

    pub fn contains_key(
        &self,
        key: &str,
    ) -> bool {
        self.locked_images.contains_key(key)
    }

    pub fn get(
        &self,
        key: &str,
    ) -> Option<&VersionedImage> {
        self.locked_images.get(key)
    }

    pub fn extend<T: IntoIterator<Item = (String, VersionedImage)>>(
        &mut self,
        iter: T,
    ) {
        self.locked_images.extend(iter);
    }

    pub fn diff(
        &self,
        other: &LockedImages,
    ) -> Vec<DiffEntry> {
        let mut diffs = Vec::new();

        for (service, new_image) in &other.locked_images {
            match self.locked_images.get(service) {
                None => {
                    diffs.push(DiffEntry::Added {
                        service: service.to_string(),
                        new: new_image.clone(),
                    });
                }
                Some(old_image) if old_image != new_image => {
                    diffs.push(DiffEntry::Updated {
                        service: service.to_string(),
                        old: old_image.clone(),
                        new: new_image.clone(),
                    });
                }
                _ => {}
            }
        }

        for (service, old_image) in &self.locked_images {
            if !other
                .locked_images
                .contains_key(service)
            {
                diffs.push(DiffEntry::Removed {
                    service: service.to_string(),
                    old: old_image.clone(),
                });
            }
        }

        diffs
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffEntry {
    Added {
        service: String,
        new: VersionedImage,
    },
    Removed {
        service: String,
        old: VersionedImage,
    },
    Updated {
        service: String,
        old: VersionedImage,
        new: VersionedImage,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(
        image: &str,
        version: &str,
        digest: &str,
    ) -> VersionedImage {
        VersionedImage {
            image: image.to_string(),
            version: Some(version.to_string()),
            digest: digest.to_string(),
        }
    }

    #[test]
    fn diff_empty_vs_empty() {
        let a = LockedImages::default();
        let b = LockedImages::default();
        assert!(a.diff(&b).is_empty());
    }

    #[test]
    fn diff_added() {
        let a = LockedImages::default();
        let mut b = LockedImages::default();
        b.insert("myapp.web".into(), img("nginx", "1.0", "sha256:aaa"));
        let diffs = a.diff(&b);
        assert_eq!(diffs.len(), 1);
        assert!(
            matches!(&diffs[0], DiffEntry::Added { service, .. } if service == "myapp.web")
        );
    }

    #[test]
    fn diff_removed() {
        let mut a = LockedImages::default();
        a.insert("myapp.web".into(), img("nginx", "1.0", "sha256:aaa"));
        let b = LockedImages::default();
        let diffs = a.diff(&b);
        assert_eq!(diffs.len(), 1);
        assert!(
            matches!(&diffs[0], DiffEntry::Removed { service, .. } if service == "myapp.web")
        );
    }

    #[test]
    fn diff_updated() {
        let mut a = LockedImages::default();
        a.insert("myapp.web".into(), img("nginx", "1.0", "sha256:aaa"));
        let mut b = LockedImages::default();
        b.insert("myapp.web".into(), img("nginx", "1.1", "sha256:bbb"));
        let diffs = a.diff(&b);
        assert_eq!(diffs.len(), 1);
        assert!(
            matches!(&diffs[0], DiffEntry::Updated { service, .. } if service == "myapp.web")
        );
    }

    #[test]
    fn diff_unchanged() {
        let mut a = LockedImages::default();
        a.insert("myapp.web".into(), img("nginx", "1.0", "sha256:aaa"));
        let mut b = LockedImages::default();
        b.insert("myapp.web".into(), img("nginx", "1.0", "sha256:aaa"));
        assert!(a.diff(&b).is_empty());
    }

    #[test]
    fn diff_mixed() {
        let mut a = LockedImages::default();
        a.insert("keep".into(), img("nginx", "1.0", "sha256:aaa"));
        a.insert("remove".into(), img("postgres", "1.0", "sha256:bbb"));
        a.insert("update".into(), img("redis", "1.0", "sha256:ccc"));

        let mut b = LockedImages::default();
        b.insert("keep".into(), img("nginx", "1.0", "sha256:aaa"));
        b.insert("add".into(), img("node", "2.0", "sha256:ddd"));
        b.insert("update".into(), img("redis", "2.0", "sha256:eee"));

        let diffs = a.diff(&b);
        assert_eq!(diffs.len(), 3);

        let added: Vec<_> = diffs
            .iter()
            .filter(|d| matches!(d, DiffEntry::Added { .. }))
            .collect();
        let removed: Vec<_> = diffs
            .iter()
            .filter(|d| matches!(d, DiffEntry::Removed { .. }))
            .collect();
        let updated: Vec<_> = diffs
            .iter()
            .filter(|d| matches!(d, DiffEntry::Updated { .. }))
            .collect();

        assert_eq!(added.len(), 1);
        assert_eq!(removed.len(), 1);
        assert_eq!(updated.len(), 1);
    }

    #[test]
    fn deserialize_full_format() {
        let json = r#"{"myapp.web":{"image":"nginx","version":"1.0","digest":"sha256:aaa"}}"#;
        let locked: LockedImages = serde_json::from_str(json).unwrap();
        assert!(locked.contains_key("myapp.web"));
        assert_eq!(locked.get("myapp.web").unwrap().digest, "sha256:aaa");
    }

    #[test]
    fn deserialize_rejects_digest_only_format() {
        let json = r#"{"myapp.web":"sha256:aaa"}"#;
        assert!(serde_json::from_str::<LockedImages>(json).is_err());
    }
}
