use anyhow::Result;
use clap::Parser;
use std::{collections::BTreeMap, fs, path::PathBuf};

use crate::commands::{handle_command, Commands};

pub use crate::projects::*;

mod commands;
mod docker;
mod progress;
mod projects;
mod spinner;

fn clap_parse_selector(s: &str) -> Result<TargetSelector, String> {
    parse_selector(s, &PROJECTS).map_err(|e| e.to_string())
}

fn clap_parse_image_selector(s: &str) -> Result<ImageSelector, String> {
    parse_image_selector(s, &PROJECTS).map_err(|e| e.to_string())
}

#[derive(Parser)]
#[command(name = "nirion")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

fn get_env_path(key: &str) -> anyhow::Result<PathBuf> {
    let val = std::env::var(key)
        .map_err(|_| anyhow::anyhow!("Env var {} must be set", key))?;
    val.parse()
        .map_err(|_| anyhow::anyhow!("Failed parsing env var {} as path", key))
}

#[tokio::main]
async fn main() -> Result<()> {
    let lock_file = get_env_path("NIRION_LOCK_FILE")?;
    let locked_images: BTreeMap<String, String> = if lock_file.exists() {
        let lock_file_data = fs::read_to_string(&lock_file)?;
        serde_json::from_str(&lock_file_data)?
    } else {
        BTreeMap::new()
    };

    let cli = Cli::parse();

    handle_command(&cli.command, &PROJECTS, &locked_images, &lock_file).await?;

    Ok(())
}
