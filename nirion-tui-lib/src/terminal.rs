use console::Term;
use std::io::Write;

pub struct HiddenCursorGuard;

impl HiddenCursorGuard {
    pub fn hide() -> anyhow::Result<Self> {
        hide_cursor()?;
        Ok(Self)
    }
}

impl Drop for HiddenCursorGuard {
    fn drop(&mut self) {
        let _ = show_cursor();
    }
}

pub fn hide_cursor() -> anyhow::Result<()> {
    let term = Term::stdout();
    term.hide_cursor()?;
    Ok(())
}

pub fn show_cursor() -> anyhow::Result<()> {
    let term = Term::stdout();
    term.show_cursor()?;
    term.flush()?;
    Ok(())
}

pub fn terminal_width() -> usize {
    Term::stdout().size().1 as usize
}

pub fn move_cursor_up(lines: usize) -> anyhow::Result<()> {
    let term = Term::stdout();
    term.move_cursor_up(lines)?;
    term.flush()?;
    Ok(())
}

pub fn move_stdout_cursor_down(lines: usize) -> anyhow::Result<()> {
    let term = Term::stdout();
    term.move_cursor_down(lines)?;
    term.flush()?;
    Ok(())
}

pub fn write_move_cursor_up(
    out: &mut impl Write,
    lines: usize,
) -> std::io::Result<()> {
    if lines > 0 {
        write!(out, "\x1b[{lines}A")?;
    }
    Ok(())
}

pub fn write_move_cursor_down(
    out: &mut impl Write,
    lines: usize,
) -> std::io::Result<()> {
    if lines > 0 {
        write!(out, "\x1b[{lines}B")?;
    }
    Ok(())
}

pub fn write_move_cursor_to_column(
    out: &mut impl Write,
    column: usize,
) -> std::io::Result<()> {
    write!(out, "\x1b[{}G", column + 1)
}

pub fn write_clear_current_line(out: &mut impl Write) -> std::io::Result<()> {
    write!(out, "\x1b[2K")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_move_cursor_up_writes_ansi_sequence() {
        let mut out = Vec::new();

        write_move_cursor_up(&mut out, 3).unwrap();

        assert_eq!(out, b"\x1b[3A");
    }

    #[test]
    fn write_move_cursor_up_skips_zero_lines() {
        let mut out = Vec::new();

        write_move_cursor_up(&mut out, 0).unwrap();

        assert!(out.is_empty());
    }

    #[test]
    fn write_move_cursor_down_writes_ansi_sequence() {
        let mut out = Vec::new();

        write_move_cursor_down(&mut out, 2).unwrap();

        assert_eq!(out, b"\x1b[2B");
    }

    #[test]
    fn write_move_cursor_down_skips_zero_lines() {
        let mut out = Vec::new();

        write_move_cursor_down(&mut out, 0).unwrap();

        assert!(out.is_empty());
    }

    #[test]
    fn write_move_cursor_to_column_writes_one_based_ansi_column() {
        let mut out = Vec::new();

        write_move_cursor_to_column(&mut out, 4).unwrap();

        assert_eq!(out, b"\x1b[5G");
    }

    #[test]
    fn write_clear_current_line_writes_ansi_sequence() {
        let mut out = Vec::new();

        write_clear_current_line(&mut out).unwrap();

        assert_eq!(out, b"\x1b[2K");
    }
}
