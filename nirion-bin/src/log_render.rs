use std::io::{Write, stderr, stdout};

use nirion_lib::logs::{LogEvent, LogLine, LogSource};
use nirion_tui_lib::color::Colorize;

use crate::commands::logs::{LogEventsMode, LogLabelFormat};

pub struct LogRenderer {
    label: LogLabelFormat,
    events: LogEventsMode,
    follow: bool,
}

enum LogLabelColor {
    Stdout,
    Stderr,
    Event,
}

impl LogRenderer {
    pub fn new(
        label: LogLabelFormat,
        events: LogEventsMode,
        follow: bool,
    ) -> Self {
        Self {
            label,
            events,
            follow,
        }
    }

    pub fn render(
        &mut self,
        event: LogEvent,
    ) -> anyhow::Result<()> {
        match event {
            LogEvent::StdoutLine(line) => self.render_stdout_line(&line),
            LogEvent::StderrLine(line) => self.render_stderr_line(&line),
            LogEvent::SourceAttached(source) => self.render_event(
                &source,
                format!("attached {}", source.container_name),
            ),
            LogEvent::SourceExited(source) => {
                let message = match source.exit_code {
                    Some(code) => format!("exited with code {code}"),
                    None => "exited".to_string(),
                };
                self.render_event(&source, message)
            }
            LogEvent::SourceDetached(source) => self.render_event(
                &source,
                format!("detached {}", source.container_name),
            ),
        }
    }

    fn render_stdout_line(
        &self,
        log_line: &LogLine,
    ) -> anyhow::Result<()> {
        let line = self.format_with_label(
            &log_line.source,
            &log_line.line,
            LogLabelColor::Stdout,
        );
        writeln!(stdout(), "{line}")?;
        Ok(())
    }

    fn render_stderr_line(
        &self,
        log_line: &LogLine,
    ) -> anyhow::Result<()> {
        let line = self.format_with_label(
            &log_line.source,
            &log_line.line,
            LogLabelColor::Stderr,
        );
        writeln!(stderr(), "{line}")?;
        Ok(())
    }

    fn render_event(
        &self,
        source: &LogSource,
        message: String,
    ) -> anyhow::Result<()> {
        if !self.show_events() {
            return Ok(());
        }

        let line =
            self.format_with_label(source, &message, LogLabelColor::Event);
        writeln!(stderr(), "{line}")?;
        Ok(())
    }

    fn show_events(&self) -> bool {
        match self.events {
            LogEventsMode::Auto => self.follow,
            LogEventsMode::Always => true,
            LogEventsMode::Never => false,
        }
    }

    fn format_with_label(
        &self,
        source: &LogSource,
        line: &str,
        color: LogLabelColor,
    ) -> String {
        let Some(label) = self.format_label(source) else {
            return line.to_string();
        };

        let label = match color {
            LogLabelColor::Stdout => label.cyan().to_string(),
            LogLabelColor::Stderr => label.yellow().to_string(),
            LogLabelColor::Event => label.magenta().to_string(),
        };
        format!("[{label}] {line}")
    }

    fn format_label(
        &self,
        source: &LogSource,
    ) -> Option<String> {
        self.format_label_parts(
            &source.project,
            &source.service,
            &source.container_name,
        )
    }

    fn format_label_parts(
        &self,
        project: &str,
        service: &str,
        container: &str,
    ) -> Option<String> {
        match self.label {
            LogLabelFormat::ProjectService => {
                if service.is_empty() {
                    Some(project.to_string())
                } else {
                    Some(format!("{project}.{service}"))
                }
            }
            LogLabelFormat::Service => {
                if service.is_empty() {
                    Some(project.to_string())
                } else {
                    Some(service.to_string())
                }
            }
            LogLabelFormat::Container => {
                if container.is_empty() {
                    Some(project.to_string())
                } else {
                    Some(container.to_string())
                }
            }
            LogLabelFormat::None => None,
        }
    }
}
