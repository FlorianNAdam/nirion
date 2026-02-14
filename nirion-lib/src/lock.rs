use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Default, Clone)]
pub struct LockedImages {
    locked_images: BTreeMap<String, String>,
}

impl<'de> Deserialize<'de> for LockedImages {
    fn deserialize<D>(deserializer: D) -> Result<LockedImages, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let locked_images =
            BTreeMap::<String, String>::deserialize(deserializer)?;

        Ok(Self { locked_images })
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
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.locked_images
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.locked_images.contains_key(key)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.locked_images
            .get(key)
            .map(|v| v.as_str())
    }

    pub fn extend<T: IntoIterator<Item = (String, String)>>(
        &mut self,
        iter: T,
    ) {
        self.locked_images.extend(iter);
    }
}
