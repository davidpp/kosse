//! Markdown (GFM) tables, the default output of agent-facing commands (blueprint/agent-surface.md).
//!
//! Cells are untrusted process- and probe-derived text: after [`printable`], backslashes, pipes
//! and backticks are escaped so a cell can neither end early, add a column or row, nor open a code
//! span. Columns are padded so the table also reads in a terminal.

use std::io::{self, Write};

use crate::text::printable;

/// What an unknown or empty value prints as.
pub const UNKNOWN: &str = "-";

/// A table with `N` columns; every row has exactly `N` cells.
pub struct Table<const N: usize> {
    headers: [&'static str; N],
    rows: Vec<[String; N]>,
}

impl<const N: usize> Table<N> {
    pub fn new(headers: [&'static str; N]) -> Self {
        Self {
            headers,
            rows: Vec::new(),
        }
    }

    /// Adds a row; each value is escaped with [`cell`].
    pub fn row(&mut self, values: [&str; N]) {
        self.rows.push(values.map(cell));
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn write(&self, out: &mut impl Write) -> io::Result<()> {
        let mut widths = self.headers.map(|header| header.chars().count().max(3));
        for row in &self.rows {
            for (width, value) in widths.iter_mut().zip(row) {
                *width = (*width).max(value.chars().count());
            }
        }
        write_line(out, &self.headers.map(str::to_owned), &widths)?;
        write_line(out, &widths.map(|width| "-".repeat(width)), &widths)?;
        for row in &self.rows {
            write_line(out, row, &widths)?;
        }
        Ok(())
    }
}

fn write_line<const N: usize>(
    out: &mut impl Write,
    cells: &[String; N],
    widths: &[usize; N],
) -> io::Result<()> {
    write!(out, "|")?;
    for (value, width) in cells.iter().zip(widths) {
        write!(out, " {value:<width$} |")?;
    }
    writeln!(out)
}

/// One escaped cell: control and invisible characters as [`printable`] escapes them, then `\`,
/// `|` and `` ` `` backslash-escaped. Empty is [`UNKNOWN`].
pub fn cell(value: &str) -> String {
    let value = printable(value.trim());
    if value.is_empty() {
        return UNKNOWN.to_owned();
    }
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, '\\' | '|' | '`') {
            out.push('\\');
        }
        out.push(character);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::Table;

    /// Splits a GFM table line on its unescaped pipes.
    fn columns(line: &str) -> usize {
        let mut count = 0_usize;
        let mut escaped = false;
        for character in line.chars() {
            match character {
                '\\' if !escaped => escaped = true,
                '|' if !escaped => count += 1,
                _ => escaped = false,
            }
        }
        count.saturating_sub(1)
    }

    #[test]
    fn hostile_cells_cannot_add_rows_columns_or_code_spans() {
        let mut table = Table::new(["PROCESS", "COMMAND"]);
        table.row(["node", "x | y\n| injected | row |"]);
        table.row(["sh", "a\\|b `rm -rf` \\"]);
        table.row(["", "\u{202E}gnp.exe"]);
        let mut out = Vec::new();
        table.write(&mut out).expect("in-memory write");
        let text = String::from_utf8(out).expect("UTF-8");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 5, "{text}");
        for line in &lines {
            assert_eq!(columns(line), 2, "{line}");
        }
        assert!(
            !lines[3].contains(" `rm"),
            "unescaped backtick: {}",
            lines[3]
        );
        assert!(
            lines[4].starts_with("| -"),
            "empty cell prints as unknown: {}",
            lines[4]
        );
        assert!(!text.contains('\u{202E}'), "bidi override passed through");
    }
}
