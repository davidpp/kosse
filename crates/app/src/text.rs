//! Process- and probe-derived text for terminals and native menus.

use std::fmt::Write as _;

/// Process names are untrusted: escape characters that can drive or disguise terminal output.
pub fn printable(text: &str) -> String {
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
