use console::Term;

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
