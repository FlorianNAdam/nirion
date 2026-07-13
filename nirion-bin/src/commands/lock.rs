use clap::Parser;
use crossterm::style::Stylize;
use futures::StreamExt;
use nirion_lib::{
    events::LockUpdateEvent,
    lock::DiffEntry,
    lock_update::update_images,
    projects::{get_images, TargetSelector},
};

use crate::{commands::NirionContext, ClapSelector};

/// Create missing lock file entries
#[derive(Parser, Debug, Clone)]
pub struct LockArgs {
    /// Target selector: *, project, or project.service
    #[arg(
        default_value = "*",
        value_parser = TargetSelector::clap_parse,
        add = TargetSelector::clap_completer()
    )]
    pub target: TargetSelector,

    /// Number of concurrent digest fetches
    #[arg(short = 'j', long = "jobs", default_value_t = 10)]
    pub jobs: usize,
}

pub async fn handle_lock(
    args: &LockArgs,
    context: &NirionContext,
) -> anyhow::Result<()> {
    let mut images = get_images(&args.target, &context.projects);
    images.retain(|name, _| !context.locked_images.contains_key(name));

    let mut operation = update_images(
        context.oci_client.clone(),
        images,
        context.locked_images.clone(),
        context.lock_file.clone(),
        args.jobs,
    );

    while let Some(event) = operation.events.next().await {
        render_lock_update_event(event?);
    }

    operation.finish().await?;

    Ok(())
}

pub fn render_lock_update_event(event: LockUpdateEvent) {
    match event {
        LockUpdateEvent::NoImages => println!("No images found to update"),
        LockUpdateEvent::ImageStarted { service, image } => {
            println!("Checking {service}: {image}");
        }
        LockUpdateEvent::ImageResolved { service } => {
            println!("Resolved {service}");
        }
        LockUpdateEvent::UpToDate => {
            println!("All images are already up-to-date")
        }
        LockUpdateEvent::ChangesDetected { diffs } => {
            println!("\nChanges:");
            print_diff(&diffs);
        }
        LockUpdateEvent::WritingLockFile => println!("\nUpdating lock file..."),
        LockUpdateEvent::LockFileWritten => {
            println!("Lock file updated successfully")
        }
    }
}

fn print_diff(diffs: &[DiffEntry]) {
    for entry in diffs {
        match entry {
            DiffEntry::Added { service, new } => {
                println!("  + {}:", service.to_string().green());
                if let Some(version) = &new.version {
                    println!("      new version: {}", version);
                }
                println!("      new digest: {}", new.digest);
            }
            DiffEntry::Updated { service, old, new } => {
                println!("  ~ {}:", service.to_string().cyan());
                if let Some(version) = &new.version {
                    let old_version = old
                        .version
                        .as_ref()
                        .map(|s| s.as_str())
                        .unwrap_or("none");

                    println!(
                        "      new version: {} -> {}",
                        old_version, version
                    );
                }
                println!("      old digest: {}", old.digest);
                println!("      new digest: {}", new.digest);
            }
            DiffEntry::Removed { service, old } => {
                println!("  - {}:", service.to_string().yellow());
                if let Some(version) = &old.version {
                    println!("      old version: {}", version);
                }
                println!("      old digest: {}", old.digest);
            }
        }
    }
}
