use crate::ansi::{ansi_len, lpad_ansi};
use crate::color::{Color, Colorize};

const DEFAULT_MAX_BAR_WIDTH: usize = 40;
const DEFAULT_MIN_BAR_WIDTH: usize = 10;
const DEFAULT_SAFETY_MARGIN: usize = 2;

// Visible width of ` │ {bar} │ ` in a status content line.
const STATUS_LINE_FIXED_WIDTH: usize = 6;

pub struct Status {
    pub entries: Vec<StatusEntry>,
    pub max_bar_width: usize,
    pub min_bar_width: usize,
    pub safety_margin: usize,
}

pub struct StatusEntry {
    pub prefix: String,
    pub segments: Vec<Color>,
    pub suffix: String,
}

impl Status {
    pub fn new(entries: Vec<StatusEntry>) -> Self {
        Self {
            entries,
            max_bar_width: DEFAULT_MAX_BAR_WIDTH,
            min_bar_width: DEFAULT_MIN_BAR_WIDTH,
            safety_margin: DEFAULT_SAFETY_MARGIN,
        }
    }

    pub fn render(
        &self,
        width: usize,
    ) -> String {
        self.render_lines(width).join("\n")
    }

    pub fn render_lines(
        &self,
        width: usize,
    ) -> Vec<String> {
        let max_prefix_width = max_entry_width(&self.entries, |e| &e.prefix);
        let max_suffix_width = max_entry_width(&self.entries, |e| &e.suffix);
        let bar_width =
            self.bar_width(width, max_prefix_width, max_suffix_width);
        let mut lines = Vec::new();

        lines.push(format!(
            "{} ┌{}┐",
            " ".repeat(max_prefix_width),
            "─".repeat(bar_width + 2)
        ));

        let num_entries = self.entries.len();
        for (i, entry) in self.entries.iter().enumerate() {
            let line = render_status_line(entry, max_prefix_width, bar_width);

            lines.push(line);

            if i != num_entries.saturating_sub(1) {
                lines.push(format!(
                    "{} ├{}┤",
                    " ".repeat(max_prefix_width),
                    "─".repeat(bar_width + 2)
                ));
            }
        }

        lines.push(format!(
            "{} └{}┘",
            " ".repeat(max_prefix_width),
            "─".repeat(bar_width + 2)
        ));
        lines
    }

    fn bar_width(
        &self,
        terminal_width: usize,
        prefix_width: usize,
        suffix_width: usize,
    ) -> usize {
        let min_bar_width = self
            .min_bar_width
            .min(self.max_bar_width);

        terminal_width
            .saturating_sub(prefix_width)
            .saturating_sub(suffix_width)
            .saturating_sub(STATUS_LINE_FIXED_WIDTH)
            .saturating_sub(self.safety_margin)
            .min(self.max_bar_width)
            .max(min_bar_width)
    }
}

fn max_entry_width(
    entries: &[StatusEntry],
    value: impl Fn(&StatusEntry) -> &str,
) -> usize {
    entries
        .iter()
        .map(value)
        .map(ansi_len)
        .max()
        .unwrap_or_default()
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
        if width == 0 {
            continue;
        }

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
    fn render_status_bar_skips_segments_without_visible_width() {
        let bar = render_status_bar(
            &[Color::Green, Color::Red, Color::Blue, Color::Yellow],
            2,
        );

        assert_eq!(strip_ansi_codes(&bar).chars().count(), 2);
        assert_eq!(strip_ansi_codes(&bar), "▊▊");
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
    fn status_bar_width_uses_available_terminal_width() {
        let status = Status::new(vec![]);

        assert_eq!(status.bar_width(80, 10, 8), DEFAULT_MAX_BAR_WIDTH);
    }

    #[test]
    fn status_bar_width_shrinks_to_available_terminal_width() {
        let status = Status::new(vec![]);
        let prefix_width = 10;
        let suffix_width = 8;
        let min_width_terminal = prefix_width
            + suffix_width
            + STATUS_LINE_FIXED_WIDTH
            + DEFAULT_SAFETY_MARGIN
            + DEFAULT_MIN_BAR_WIDTH;

        assert_eq!(
            status.bar_width(min_width_terminal, prefix_width, suffix_width),
            DEFAULT_MIN_BAR_WIDTH,
        );
        assert_eq!(
            status.bar_width(
                min_width_terminal + 1,
                prefix_width,
                suffix_width,
            ),
            DEFAULT_MIN_BAR_WIDTH + 1,
        );
    }

    #[test]
    fn status_bar_width_keeps_minimum_width() {
        let status = Status::new(vec![]);

        assert_eq!(status.bar_width(10, 10, 8), DEFAULT_MIN_BAR_WIDTH);
    }

    #[test]
    fn render_status_line_pads_prefix_to_visible_width() {
        let entry = StatusEntry {
            prefix: "db".to_string(),
            segments: vec![],
            suffix: "ready".to_string(),
        };

        let line = render_status_line(&entry, 4, 3);
        let line = strip_ansi_codes(&line);
        let parts = line.split('│').collect::<Vec<_>>();

        assert_eq!(parts[0].chars().count(), 5);
        assert!(parts[0].starts_with("db"));
        assert_eq!(parts[2].trim(), "ready");
    }

    #[test]
    fn render_lines_returns_complete_status_frame() {
        let status = Status::new(vec![StatusEntry {
            prefix: "db".to_string(),
            segments: vec![],
            suffix: "ready".to_string(),
        }]);

        let lines = status.render_lines(usize::MAX);

        assert_eq!(lines.len(), 3);
        assert_status_box(&lines, "db ", " ready");
    }

    #[test]
    fn render_lines_with_width_shrinks_status_bar() {
        let status = Status::new(vec![StatusEntry {
            prefix: "db".to_string(),
            segments: vec![],
            suffix: "ready".to_string(),
        }]);

        let width = 20;
        let lines = status.render_lines(width);
        let line = strip_ansi_codes(&lines[1]);
        let bar_area = line.split('│').nth(1).unwrap();
        let expected_bar_width = width
            .saturating_sub(ansi_len("db"))
            .saturating_sub(ansi_len("ready"))
            .saturating_sub(STATUS_LINE_FIXED_WIDTH)
            .saturating_sub(DEFAULT_SAFETY_MARGIN)
            .min(DEFAULT_MAX_BAR_WIDTH)
            .max(DEFAULT_MIN_BAR_WIDTH);

        assert_eq!(bar_area.chars().count() - 2, expected_bar_width);
    }

    #[test]
    fn render_lines_separates_multiple_status_entries() {
        let status = Status::new(vec![
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
        ]);

        let lines = status.render_lines(usize::MAX);

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
