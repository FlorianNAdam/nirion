use crossterm::{
    cursor::MoveToColumn,
    execute,
    style::{Color, Stylize},
};
use std::io::stdout;

use crate::util::{ansi_len, lpad_ansi};

pub struct Status {
    pub entries: Vec<StatusEntry>,
}

pub struct StatusEntry {
    pub prefix: String,
    pub segments: Vec<Color>,
    pub suffix: String,
}

impl Status {
    pub fn print(self) -> anyhow::Result<()> {
        let bar_width = 40;

        let max_prefix_length = self
            .entries
            .iter()
            .map(|e| ansi_len(&e.prefix))
            .max()
            .unwrap_or_default();

        let mut stdout = stdout();

        println!(
            "{} ┌{}┐",
            " ".repeat(max_prefix_length),
            "─".repeat(bar_width + 2)
        );

        let num_entries = self.entries.len();
        for (i, entry) in self.entries.into_iter().enumerate() {
            let line = render_status_line(entry, max_prefix_length, bar_width);

            execute!(stdout, MoveToColumn(0))?;
            println!("{}", line);

            if i != num_entries.saturating_sub(1) {
                println!(
                    "{} ├{}┤",
                    " ".repeat(max_prefix_length),
                    "─".repeat(bar_width + 2)
                )
            }
        }

        println!(
            "{} └{}┘",
            " ".repeat(max_prefix_length),
            "─".repeat(bar_width + 2)
        );
        Ok(())
    }
}

fn render_status_line(
    entry: StatusEntry,
    max_prefix_width: usize,
    bar_width: usize,
) -> String {
    let prefix = lpad_ansi(&entry.prefix, max_prefix_width);
    let bar = render_status_bar(entry.segments, bar_width);
    let suffix = entry.suffix;

    format!("{prefix} │ {bar} │ {suffix}")
}

fn render_status_bar(segments: Vec<Color>, width: usize) -> String {
    if segments.len() == 0 {
        return " ".repeat(width);
    }

    let mut out = String::new();
    for (i, color) in segments.iter().enumerate() {
        let width = optimal_sublist_length(width, segments.len(), i);

        out.push_str(
            "█"
                .repeat(width.saturating_sub(1))
                .with(*color)
                .to_string()
                .as_str(),
        );
        out.push_str("▊".with(*color).to_string().as_str());
    }
    out
}

fn optimal_sublist_length(width: usize, n: usize, i: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let base = width / n;
    let extra = width % n;

    if i < extra {
        base + 1
    } else {
        base
    }
}
