use crate::ansi::{ansi_len, lpad_ansi};
use crate::color::{Color, Colorize};

pub struct Status {
    pub entries: Vec<StatusEntry>,
}

pub struct StatusEntry {
    pub prefix: String,
    pub segments: Vec<Color>,
    pub suffix: String,
}

impl Status {
    pub fn print(&self) -> anyhow::Result<()> {
        println!("{}", self.render());
        Ok(())
    }

    pub fn render(&self) -> String {
        self.render_lines().join("\n")
    }

    pub fn render_lines(&self) -> Vec<String> {
        let bar_width = 40;

        let max_prefix_length = self
            .entries
            .iter()
            .map(|e| ansi_len(&e.prefix))
            .max()
            .unwrap_or_default();

        let mut lines = Vec::new();

        lines.push(format!(
            "{} ┌{}┐",
            " ".repeat(max_prefix_length),
            "─".repeat(bar_width + 2)
        ));

        let num_entries = self.entries.len();
        for (i, entry) in self.entries.iter().enumerate() {
            let line = render_status_line(entry, max_prefix_length, bar_width);

            lines.push(line);

            if i != num_entries.saturating_sub(1) {
                lines.push(format!(
                    "{} ├{}┤",
                    " ".repeat(max_prefix_length),
                    "─".repeat(bar_width + 2)
                ));
            }
        }

        lines.push(format!(
            "{} └{}┘",
            " ".repeat(max_prefix_length),
            "─".repeat(bar_width + 2)
        ));
        lines
    }
}

fn render_status_line(
    entry: &StatusEntry,
    max_prefix_width: usize,
    bar_width: usize,
) -> String {
    let prefix = lpad_ansi(&entry.prefix, max_prefix_width);
    let bar = render_status_bar(&entry.segments, bar_width);
    let suffix = &entry.suffix;

    format!("{prefix} │ {bar} │ {suffix}")
}

fn render_status_bar(
    segments: &[Color],
    width: usize,
) -> String {
    if segments.is_empty() {
        return " ".repeat(width);
    }

    let mut out = String::new();
    for (i, color) in segments.iter().enumerate() {
        let width = optimal_sublist_length(width, segments.len(), i);

        out.push_str(
            "█"
                .repeat(width.saturating_sub(1))
                .fg(*color)
                .to_string()
                .as_str(),
        );
        out.push_str("▊".fg(*color).to_string().as_str());
    }
    out
}

fn optimal_sublist_length(
    width: usize,
    n: usize,
    i: usize,
) -> usize {
    if n == 0 {
        return 0;
    }
    let base = width / n;
    let extra = width % n;

    if i < extra { base + 1 } else { base }
}

#[cfg(test)]
mod tests {
    use super::*;
    use console::strip_ansi_codes;

    #[test]
    fn render_status_bar_returns_spaces_for_no_segments() {
        assert_eq!(render_status_bar(&[], 4), "    ");
    }

    #[test]
    fn render_status_bar_preserves_requested_visible_width() {
        let bar =
            render_status_bar(&[Color::Green, Color::Red, Color::Blue], 8);

        assert_eq!(strip_ansi_codes(&bar).chars().count(), 8);
        assert_eq!(strip_ansi_codes(&bar), "██▊██▊█▊");
    }

    #[test]
    fn optimal_sublist_length_distributes_remainder_to_first_segments() {
        assert_eq!(optimal_sublist_length(8, 3, 0), 3);
        assert_eq!(optimal_sublist_length(8, 3, 1), 3);
        assert_eq!(optimal_sublist_length(8, 3, 2), 2);
    }

    #[test]
    fn optimal_sublist_length_returns_zero_for_empty_sublist() {
        assert_eq!(optimal_sublist_length(8, 0, 0), 0);
    }

    #[test]
    fn render_status_line_pads_prefix_to_visible_width() {
        let entry = StatusEntry {
            prefix: "db".to_string(),
            segments: vec![],
            suffix: "ready".to_string(),
        };

        assert_eq!(render_status_line(&entry, 4, 3), "db   │     │ ready");
    }

    #[test]
    fn render_lines_returns_complete_status_frame() {
        let status = Status {
            entries: vec![StatusEntry {
                prefix: "db".to_string(),
                segments: vec![],
                suffix: "ready".to_string(),
            }],
        };

        let lines = status.render_lines();

        assert_eq!(lines.len(), 3);
        assert_status_box(&lines, "db ", " ready");
    }

    #[test]
    fn render_lines_separates_multiple_status_entries() {
        let status = Status {
            entries: vec![
                StatusEntry {
                    prefix: "web".to_string(),
                    segments: vec![],
                    suffix: "starting".to_string(),
                },
                StatusEntry {
                    prefix: "db".to_string(),
                    segments: vec![],
                    suffix: "ready".to_string(),
                },
            ],
        };

        let lines = status.render_lines();

        assert_eq!(lines.len(), 5);
        let left = lines[0]
            .chars()
            .position(|c| c == '┌')
            .unwrap();
        let right = lines[0]
            .chars()
            .position(|c| c == '┐')
            .unwrap();

        assert_eq!(lines[1].chars().nth(left), Some('│'));
        assert_eq!(lines[1].chars().nth(right), Some('│'));
        assert_eq!(lines[2].chars().nth(left), Some('├'));
        assert_eq!(lines[2].chars().nth(right), Some('┤'));
        assert_eq!(lines[3].chars().nth(left), Some('│'));
        assert_eq!(lines[3].chars().nth(right), Some('│'));
        assert_eq!(lines[4].chars().nth(left), Some('└'));
        assert_eq!(lines[4].chars().nth(right), Some('┘'));
        assert!(lines[1].starts_with("web "));
        assert!(lines[1].ends_with(" starting"));
        assert!(lines[3].starts_with("db  "));
        assert!(lines[3].ends_with(" ready"));
    }

    #[test]
    fn print_writes_rendered_status() {
        let status = Status { entries: vec![] };

        status.print().unwrap();
    }

    fn assert_status_box(
        lines: &[String],
        content_prefix: &str,
        content_suffix: &str,
    ) {
        const TOP_LEFT: char = '┌';
        const TOP_RIGHT: char = '┐';
        const BOTTOM_LEFT: char = '└';
        const BOTTOM_RIGHT: char = '┘';
        const VERTICAL: char = '│';
        const HORIZONTAL: char = '─';

        let top = &lines[0];
        let content = &lines[1];
        let bottom = &lines[2];
        let left = top
            .chars()
            .position(|c| c == TOP_LEFT)
            .unwrap();
        let right = top
            .chars()
            .position(|c| c == TOP_RIGHT)
            .unwrap();

        assert_eq!(top.chars().nth(left), Some(TOP_LEFT));
        assert_eq!(top.chars().nth(right), Some(TOP_RIGHT));
        assert_eq!(bottom.chars().nth(left), Some(BOTTOM_LEFT));
        assert_eq!(bottom.chars().nth(right), Some(BOTTOM_RIGHT));
        assert_eq!(content.chars().nth(left), Some(VERTICAL));
        assert_eq!(content.chars().nth(right), Some(VERTICAL));
        assert!(top.chars().take(left).all(|c| c == ' '));
        assert!(
            bottom
                .chars()
                .take(left)
                .all(|c| c == ' ')
        );
        assert!(
            top.chars()
                .skip(left + 1)
                .take(right - left - 1)
                .all(|c| c == HORIZONTAL)
        );
        assert!(
            bottom
                .chars()
                .skip(left + 1)
                .take(right - left - 1)
                .all(|c| c == HORIZONTAL)
        );
        assert!(content.starts_with(content_prefix));
        assert!(content.ends_with(content_suffix));
    }
}
