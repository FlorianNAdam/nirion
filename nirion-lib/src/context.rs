use std::{path::PathBuf, sync::Arc};

use nirion_oci_lib::client::NirionOciClient;

use crate::{docker::DockerCommand, lock::LockedImages, projects::Projects};

#[derive(Clone)]
pub struct NirionContext {
    pub projects: Projects,
    pub locked_images: LockedImages,
    pub lock_file: PathBuf,
    pub oci_client: Arc<NirionOciClient>,
    pub docker_command: DockerCommand,
}
