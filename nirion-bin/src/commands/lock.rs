use clap::Parser;
use crossterm::style::Stylize;
use futures::StreamExt;
use nirion_lib::{
    context::NirionContext,
    events::LockUpdateEvent,
    lock::DiffEntry,
    lock_update::update_images,
    projects::{get_images, TargetSelector},
};

use crate::ClapSelector;

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

    let mut operation = update_images(context, images, args.jobs);

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
            print!("{}", format_diff(&diffs));
        }
        LockUpdateEvent::WritingLockFile => println!("\nUpdating lock file..."),
        LockUpdateEvent::LockFileWritten => {
            println!("Lock file updated successfully")
        }
    }
}

fn format_diff(diffs: &[DiffEntry]) -> String {
    let mut output = String::new();

    for entry in diffs {
        match entry {
            DiffEntry::Added { service, new } => {
                output.push_str(&format!(
                    "  + {}:\n",
                    service.to_string().green()
                ));
                if let Some(version) = &new.version {
                    output
                        .push_str(&format!("      new version: {}\n", version));
                }
                output.push_str(&format!("      new digest: {}\n", new.digest));
            }
            DiffEntry::Updated { service, old, new } => {
                output.push_str(&format!(
                    "  ~ {}:\n",
                    service.to_string().cyan()
                ));
                if let Some(version) = &new.version {
                    let old_version = old
                        .version
                        .as_ref()
                        .map(|s| s.as_str())
                        .unwrap_or("none");

                    output.push_str(&format!(
                        "      new version: {} -> {}",
                        old_version, version
                    ));
                    output.push('\n');
                }
                output.push_str(&format!("      old digest: {}\n", old.digest));
                output.push_str(&format!("      new digest: {}\n", new.digest));
            }
            DiffEntry::Removed { service, old } => {
                output.push_str(&format!(
                    "  - {}:\n",
                    service.to_string().yellow()
                ));
                if let Some(version) = &old.version {
                    output
                        .push_str(&format!("      old version: {}\n", version));
                }
                output.push_str(&format!("      old digest: {}\n", old.digest));
            }
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use console::strip_ansi_codes;
    use nirion_lib::lock::VersionedImage;

    fn image(
        image: &str,
        version: Option<&str>,
        digest: &str,
    ) -> VersionedImage {
        VersionedImage {
            image: image.to_string(),
            version: version.map(str::to_string),
            digest: digest.to_string(),
        }
    }

    #[test]
    fn format_diff_includes_added_updated_and_removed_changes() {
        let diffs = vec![
            DiffEntry::Added {
                service: "app.web".to_string(),
                new: image("nginx:1.27", Some("1.27"), "sha256:added"),
            },
            DiffEntry::Updated {
                service: "app.worker".to_string(),
                old: image("worker:1", Some("1.0"), "sha256:old"),
                new: image("worker:2", Some("2.0"), "sha256:new"),
            },
            DiffEntry::Removed {
                service: "app.db".to_string(),
                old: image("postgres:16", Some("16"), "sha256:removed"),
            },
        ];

        let output = strip_ansi_codes(&format_diff(&diffs)).into_owned();

        let added = output.find("+ app.web").unwrap();
        let added_version = output.find("1.27").unwrap();
        let updated = output.find("~ app.worker").unwrap();
        let old_version = output.find("1.0").unwrap();
        let new_version = output.find("2.0").unwrap();
        let removed = output.find("- app.db").unwrap();
        let removed_version = output.find("16").unwrap();

        assert!(added < added_version);
        assert!(added_version < updated);
        assert!(updated < old_version);
        assert!(old_version < new_version);
        assert!(new_version < removed);
        assert!(removed < removed_version);
    }
}
