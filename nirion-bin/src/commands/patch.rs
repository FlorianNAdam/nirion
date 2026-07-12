use anyhow::Result;
use clap::{Parser, ValueEnum};
use nirion_lib::{
    lock::LockedImages,
    patch::{patch_target, PatchTarget as LibPatchTarget},
    projects::Projects,
};
use nirion_oci_lib::client::AuthConfig;
use std::path::Path;

use crate::{ClapSelector, TargetSelector};

/// Patch service files using mirage-patch
#[derive(Parser, Debug, Clone)]
pub struct PatchArgs {
    /// Target selector: *, project, or project.service
    #[arg(
        default_value = "*",
        value_parser = TargetSelector::clap_parse,
        add = TargetSelector::clap_completer()
    )]
    pub target: TargetSelector,

    /// What to patch
    #[arg(short, long, value_enum, default_value = "compose")]
    patch_target: PatchTarget,
}

#[derive(Clone, Debug, ValueEnum, PartialEq, Eq)]
enum PatchTarget {
    EnvFile,
    Compose,
}

impl From<&PatchTarget> for LibPatchTarget {
    fn from(value: &PatchTarget) -> Self {
        match value {
            PatchTarget::EnvFile => Self::EnvFile,
            PatchTarget::Compose => Self::Compose,
        }
    }
}

pub async fn handle_patch(
    args: &PatchArgs,
    projects: &Projects,
    _locked_images: &LockedImages,
    _lock_file: &Path,
    _auth: &AuthConfig,
) -> Result<()> {
    patch_target(
        &args.target,
        projects,
        &LibPatchTarget::from(&args.patch_target),
    )
    .await
}
