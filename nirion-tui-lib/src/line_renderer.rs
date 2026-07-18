use std::io::{Write, stdout};

#[derive(Default)]
pub struct LineRenderer {
    current: Vec<String>,
    started: bool,
}

impl LineRenderer {
    pub fn start(
        &mut self,
        wanted: &str,
    ) -> anyhow::Result<()> {
        let mut stdout = stdout();
        self.start_with_writer(wanted, &mut stdout)
    }

    fn start_with_writer(
        &mut self,
        wanted: &str,
        out: &mut impl Write,
    ) -> anyhow::Result<()> {
        let wanted = split_lines(wanted);

        for line in &wanted {
            writeln!(out, "{line}")?;
        }

        if !wanted.is_empty() {
            move_up(out, wanted.len())?;
            move_to_column(out, 0)?;
        }
        out.flush()?;

        self.current = wanted;
        self.started = true;
        Ok(())
    }

    pub fn render(
        &mut self,
        wanted: &str,
    ) -> anyhow::Result<()> {
        let mut stdout = stdout();
        self.render_with_writer(wanted, &mut stdout)
    }

    fn render_with_writer(
        &mut self,
        wanted: &str,
        out: &mut impl Write,
    ) -> anyhow::Result<()> {
        if !self.started {
            return self.start_with_writer(wanted, out);
        }

        let wanted = split_lines(wanted);
        self.render_lines(&wanted, out)?;
        self.current = wanted;
        Ok(())
    }

    pub fn finish(
        &mut self,
        wanted: &str,
    ) -> anyhow::Result<()> {
        let mut stdout = stdout();
        self.finish_with_writer(wanted, &mut stdout)
    }

    fn finish_with_writer(
        &mut self,
        wanted: &str,
        out: &mut impl Write,
    ) -> anyhow::Result<()> {
        self.render_with_writer(wanted, out)?;

        if !self.current.is_empty() {
            move_down(out, self.current.len())?;
            move_to_column(out, 0)?;
        }
        out.flush()?;

        self.started = false;
        Ok(())
    }

    fn render_lines(
        &self,
        wanted: &[String],
        out: &mut impl Write,
    ) -> anyhow::Result<()> {
        let line_count = self.current.len().max(wanted.len());

        for row in 0..line_count {
            let current = self.current.get(row);
            let wanted = wanted.get(row);

            if current == wanted {
                continue;
            }

            if row > 0 {
                move_down(out, row)?;
            }
            move_to_column(out, 0)?;
            clear_current_line(out)?;
            if let Some(line) = wanted {
                write!(out, "{line}")?;
            }
            if row > 0 {
                move_up(out, row)?;
            }
            move_to_column(out, 0)?;
        }

        out.flush()?;
        Ok(())
    }
}

fn move_up(
    out: &mut impl Write,
    lines: usize,
) -> std::io::Result<()> {
    if lines > 0 {
        write!(out, "\x1b[{lines}A")?;
    }
    Ok(())
}

fn move_down(
    out: &mut impl Write,
    lines: usize,
) -> std::io::Result<()> {
    if lines > 0 {
        write!(out, "\x1b[{lines}B")?;
    }
    Ok(())
}

fn move_to_column(
    out: &mut impl Write,
    column: usize,
) -> std::io::Result<()> {
    write!(out, "\x1b[{}G", column + 1)
}

fn clear_current_line(out: &mut impl Write) -> std::io::Result<()> {
    write!(out, "\x1b[2K")
}

fn split_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.split('\n')
            .map(str::to_string)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_lines_preserves_trailing_empty_line() {
        assert_eq!(split_lines("a\n"), vec!["a".to_string(), String::new()]);
    }

    #[test]
    fn split_lines_returns_no_lines_for_empty_text() {
        assert_eq!(split_lines(""), Vec::<String>::new());
    }

    #[test]
    fn start_tracks_current_lines_and_started_state() {
        let mut renderer = LineRenderer::default();
        let mut out = Vec::new();

        renderer
            .start_with_writer("one\ntwo", &mut out)
            .unwrap();

        assert!(renderer.started);
        assert_eq!(renderer.current, vec!["one", "two"]);
    }

    #[test]
    fn render_starts_renderer_when_needed() {
        let mut renderer = LineRenderer::default();
        let mut out = Vec::new();

        renderer
            .render_with_writer("first", &mut out)
            .unwrap();

        assert!(renderer.started);
        assert_eq!(renderer.current, vec!["first"]);
    }

    #[test]
    fn render_updates_current_lines_after_diffing() {
        let mut renderer = LineRenderer::default();
        let mut out = Vec::new();
        renderer
            .start_with_writer("one\ntwo\nthree", &mut out)
            .unwrap();

        renderer
            .render_with_writer("one\nchanged", &mut out)
            .unwrap();

        assert!(renderer.started);
        assert_eq!(renderer.current, vec!["one", "changed"]);
    }

    #[test]
    fn finish_renders_final_lines_and_stops_renderer() {
        let mut renderer = LineRenderer::default();
        let mut out = Vec::new();
        renderer
            .start_with_writer("before", &mut out)
            .unwrap();

        renderer
            .finish_with_writer("after", &mut out)
            .unwrap();

        assert!(!renderer.started);
        assert_eq!(renderer.current, vec!["after"]);
    }

    #[test]
    fn public_methods_handle_empty_output() {
        let mut renderer = LineRenderer::default();

        renderer.start("").unwrap();
        assert!(renderer.started);
        assert!(renderer.current.is_empty());

        renderer.render("").unwrap();
        assert!(renderer.started);
        assert!(renderer.current.is_empty());

        renderer.finish("").unwrap();
        assert!(!renderer.started);
        assert!(renderer.current.is_empty());
    }
}
