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
