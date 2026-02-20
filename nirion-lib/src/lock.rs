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
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Full(BTreeMap<String, VersionedImage>),
            DigestOnly(BTreeMap<String, String>),
        }

        match Repr::deserialize(deserializer)? {
            Repr::Full(map) => Ok(Self { locked_images: map }),
            Repr::DigestOnly(map) => {
                let locked_images = map
                    .into_iter()
                    .map(|(k, digest)| {
                        (
                            k,
                            VersionedImage {
                                image: "<unknown>".to_string(),
                                version: None,
                                digest,
                            },
                        )
                    })
                    .collect();

                Ok(Self { locked_images })
            }
        }
    }
}

impl Serialize for LockedImages {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
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

    pub fn insert(&mut self, key: String, image: VersionedImage) {
        self.locked_images.insert(key, image);
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.locked_images.contains_key(key)
    }

    pub fn get(&self, key: &str) -> Option<&VersionedImage> {
        self.locked_images.get(key)
    }

    pub fn extend<T: IntoIterator<Item = (String, VersionedImage)>>(
        &mut self,
        iter: T,
    ) {
        self.locked_images.extend(iter);
    }

    pub fn diff(&self, other: &LockedImages) -> Vec<DiffEntry> {
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
