use console::strip_ansi_codes;

pub fn ansi_len(ansi_str: &str) -> usize {
    strip_ansi_codes(ansi_str)
        .chars()
        .count()
}

pub fn lpad_ansi(ansi_str: &str, len: usize) -> String {
    let stripped_len = ansi_len(ansi_str);
    let mut padded = ansi_str.to_string();
    padded.push_str(&" ".repeat(len.saturating_sub(stripped_len)));
    padded
}
