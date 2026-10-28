//! `stackscan list`: one scan, printed as a table or as the JSON snapshot contract.
//!
//! There is no resident instance to ask yet, so this always runs its own one-shot scan.

use std::{
    fmt::Write as _,
    io::{self, Write},
};

use stackscan_core::{model::Snapshot, scan::Scanner, sockets::SocketsError};

use crate::cli::ListArgs;

#[derive(Debug, thiserror::Error)]
pub enum ListError {
    #[error(transparent)]
    Scan(#[from] SocketsError),
    #[error("could not write the listing: {0}")]
    Write(#[source] io::Error),
}

pub fn run(args: &ListArgs) -> Result<(), ListError> {
    let snapshot = Scanner::new().scan()?;
    let mut stdout = io::stdout().lock();
    let written = if args.json {
        write_json(&mut stdout, &snapshot)
    } else {
        write_table(&mut stdout, &snapshot)
    };
    match written.and_then(|()| stdout.flush()) {
        // A closed pipe (`stackscan list | head`) just means the reader has seen enough.
        Err(error) if error.kind() != io::ErrorKind::BrokenPipe => {
            return Err(ListError::Write(error));
        }
        _ => {}
    }
    if !args.json {
        for warning in &snapshot.warnings {
            report!("stackscan: warning: {warning}");
        }
    }
    Ok(())
}

fn write_json(out: &mut impl Write, snapshot: &Snapshot) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *out, snapshot)?;
    out.write_all(b"\n")
}

fn write_table(out: &mut impl Write, snapshot: &Snapshot) -> io::Result<()> {
    writeln!(
        out,
        "{:<8} {:<24} {:<6} ADDRESSES",
        "PID", "PROCESS", "PORT"
    )?;
    for service in &snapshot.services {
        let name = service
            .process
            .name
            .as_deref()
            .map_or_else(|| "?".to_owned(), printable);
        for endpoint in &service.endpoints {
            writeln!(
                out,
                "{:<8} {:<24} {:<6} {}",
                service.process.identity.pid,
                name,
                endpoint.port,
                endpoint.bind_addresses.join(", ")
            )?;
        }
    }
    Ok(())
}

/// Process names are untrusted: escape characters that can drive or disguise terminal output.
fn printable(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if character.is_control() || is_invisible_format(character) {
            let _ = write!(out, "{}", character.escape_unicode());
        } else {
            out.push(character);
        }
    }
    out
}

/// Format characters that reorder or hide text: bidi embeddings, overrides and isolates
/// (Trojan Source), zero-width characters, line/paragraph separators and the BOM. std has no
/// Unicode general-category lookup, so the relevant ranges are listed.
fn is_invisible_format(character: char) -> bool {
    matches!(
        character,
        '\u{061C}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

#[cfg(test)]
mod tests {
    use super::printable;

    #[test]
    fn control_and_invisible_format_characters_are_escaped() {
        assert_eq!(printable("node\u{1b}[2Jx\n"), "node\\u{1b}[2Jx\\u{a}");
        assert_eq!(
            printable("a\u{202E}b\u{2066}c\u{200B}d"),
            "a\\u{202e}b\\u{2066}c\\u{200b}d"
        );
        assert_eq!(printable("vite é"), "vite é");
    }
}
