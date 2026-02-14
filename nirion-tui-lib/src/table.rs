use crate::ansi::{ansi_len, lpad_ansi};

pub fn print_table(lines: Vec<String>) {
    let split_lines: Vec<Vec<&str>> = lines
        .iter()
        .map(|line| line.split('\t').collect())
        .collect();

    let num_cols = split_lines
        .iter()
        .map(|cols| cols.len())
        .max()
        .unwrap_or(0);

    let mut col_widths = vec![0; num_cols];
    for cols in &split_lines {
        for (i, col) in cols.iter().enumerate() {
            let visible_len = ansi_len(col);
            if visible_len > col_widths[i] {
                col_widths[i] = visible_len;
            }
        }
    }

    for cols in split_lines {
        for (i, col) in cols.iter().enumerate() {
            if i < cols.len() - 1 {
                let padded = lpad_ansi(col, col_widths[i]);
                print!("{}  ", padded);
            } else {
                print!("{}", col)
            }
        }
        println!();
    }
}
