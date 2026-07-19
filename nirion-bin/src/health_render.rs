use nirion_lib::health::{
    HealthLogEntry, HealthLogEvent, HealthLogRecord, HealthLogSnapshot,
    HealthLogSource,
};
use nirion_tui_lib::color::Colorize;
use std::io::{Write, stdout};
use std::time::{Duration, SystemTime};

pub struct HealthRenderer;

#[derive(Clone, Copy)]
enum HealthLabelColor {
    Success,
    Failed,
    Unknown,
}

impl HealthRenderer {
    pub fn new() -> Self {
        Self
    }

    pub fn render(
        &mut self,
        event: HealthLogEvent,
    ) -> anyhow::Result<()> {
        match event {
            HealthLogEvent::LogEntry(record) => {
                self.render_log_record(&record)?
            }
            HealthLogEvent::NoEntries(snapshot) => {
                self.render_no_entries(&snapshot)?
            }
        }
        Ok(())
    }

    fn render_no_entries(
        &self,
        snapshot: &HealthLogSnapshot,
    ) -> anyhow::Result<()> {
        let status = snapshot
            .status
            .as_deref()
            .unwrap_or("unknown");
        writeln!(
            stdout(),
            "[{}] {status} no healthcheck log entries",
            health_label(&snapshot.source, HealthLabelColor::Unknown),
        )?;
        Ok(())
    }

    fn render_log_record(
        &self,
        record: &HealthLogRecord,
    ) -> anyhow::Result<()> {
        let source = &record.source;
        let entry = &record.entry;
        let output = entry.output.trim_end();
        let mut lines = output.lines();
        let first_line = lines.next().unwrap_or("");
        let color = if entry.exit_code == 0 {
            HealthLabelColor::Success
        } else {
            HealthLabelColor::Failed
        };
        writeln!(
            stdout(),
            "[{}] {} exit={} {}",
            health_label(source, color),
            format_healthcheck_time(entry),
            entry.exit_code,
            first_line
        )?;

        for line in lines {
            writeln!(stdout(), "  {line}")?;
        }
        Ok(())
    }
}

fn format_healthcheck_time(entry: &HealthLogEntry) -> String {
    let start = format_timestamp(entry.start);
    let duration = healthcheck_duration(entry)
        .map(format_duration)
        .unwrap_or_else(|| "?".to_string());
    format!("{start} {duration}")
}

fn format_timestamp(timestamp: SystemTime) -> String {
    let timestamp = humantime::format_rfc3339_seconds(timestamp).to_string();
    let Some((date, time)) = timestamp.split_once('T') else {
        return timestamp;
    };
    let time = time.trim_end_matches('Z');
    let time = time
        .split_once('.')
        .map(|(time, _)| time)
        .unwrap_or(time);
    format!("{date} {time}")
}

fn healthcheck_duration(entry: &HealthLogEntry) -> Option<Duration> {
    entry
        .end
        .duration_since(entry.start)
        .ok()
}

fn format_duration(duration: Duration) -> String {
    if duration.as_secs() == 0 {
        format!("{}ms", duration.as_millis())
    } else if duration.subsec_millis() == 0 {
        format!("{}s", duration.as_secs())
    } else {
        format!("{}.{:03}s", duration.as_secs(), duration.subsec_millis())
    }
}

fn health_label(
    source: &HealthLogSource,
    color: HealthLabelColor,
) -> String {
    let label = format!("{}.{}", source.project, source.service);
    match color {
        HealthLabelColor::Success => label.green().to_string(),
        HealthLabelColor::Failed => label.red().to_string(),
        HealthLabelColor::Unknown => label.yellow().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nirion_tui_lib::ansi::strip_ansi_codes;
    use std::time::UNIX_EPOCH;

    fn source() -> HealthLogSource {
        HealthLogSource {
            project: "project".to_string(),
            service: "service".to_string(),
            container_id: "abc".to_string(),
            container_name: "container".to_string(),
        }
    }

    fn entry(
        exit_code: i64,
        output: &str,
    ) -> HealthLogEntry {
        HealthLogEntry {
            start: UNIX_EPOCH + Duration::from_secs(1_789_533_615),
            end: UNIX_EPOCH + Duration::from_secs(1_789_533_616),
            exit_code,
            output: output.to_string(),
        }
    }

    #[test]
    fn format_duration_supports_milliseconds_seconds_and_fractional_seconds() {
        assert_eq!(format_duration(Duration::from_millis(250)), "250ms");
        assert_eq!(format_duration(Duration::from_secs(2)), "2s");
        assert_eq!(format_duration(Duration::from_millis(1250)), "1.250s");
    }

    #[test]
    fn format_healthcheck_time_formats_timestamp_and_duration() {
        assert_eq!(
            format_healthcheck_time(&entry(0, "OK\n")),
            "2026-09-16 04:40:15 1s"
        );
    }

    #[test]
    fn healthcheck_duration_handles_reversed_times() {
        let entry = HealthLogEntry {
            start: UNIX_EPOCH + Duration::from_secs(2),
            end: UNIX_EPOCH + Duration::from_secs(1),
            exit_code: 1,
            output: String::new(),
        };

        assert_eq!(healthcheck_duration(&entry), None);
        assert_eq!(format_healthcheck_time(&entry), "1970-01-01 00:00:02 ?");
    }

    #[test]
    fn health_label_formats_source_for_all_colors() {
        for color in [
            HealthLabelColor::Success,
            HealthLabelColor::Failed,
            HealthLabelColor::Unknown,
        ] {
            assert_eq!(
                strip_ansi_codes(&health_label(&source(), color)),
                "project.service"
            );
        }
    }

    #[test]
    fn renderer_handles_log_records_and_empty_snapshots() {
        let mut renderer = HealthRenderer::new();
        let source = source();

        renderer
            .render(HealthLogEvent::LogEntry(HealthLogRecord {
                source: source.clone(),
                status: Some("healthy".to_string()),
                entry: entry(0, "OK\n"),
            }))
            .unwrap();
        renderer
            .render(HealthLogEvent::LogEntry(HealthLogRecord {
                source: source.clone(),
                status: Some("unhealthy".to_string()),
                entry: entry(1, "failed\nmore detail\n"),
            }))
            .unwrap();
        renderer
            .render(HealthLogEvent::NoEntries(HealthLogSnapshot {
                source,
                status: None,
                entries: Vec::new(),
            }))
            .unwrap();
    }
}
