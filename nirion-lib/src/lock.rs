use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Default)]
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
