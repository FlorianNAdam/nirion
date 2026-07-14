use anyhow::Result;
use clap::{Parser, ValueEnum};
use nirion_lib::{
    context::NirionContext,
    patch::{patch_target, PatchTarget as LibPatchTarget},
};

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
    context: &NirionContext,
) -> Result<()> {
    patch_target(
        &args.target,
        &context.projects,
        &LibPatchTarget::from(&args.patch_target),
    )
    .await
}
