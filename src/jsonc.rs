//! JSONC — JSON plus `//` and `/* */` comments — the format of the recipe files a user
//! edits (`docs/design/roll-workflow.md`, "Authored files"). Every JSON document is valid
//! JSONC, so reading one through [`strip_comments`] changes nothing. Trailing commas stay
//! invalid. Comments are never preserved: a round trip through serde drops them, which is
//! why Hanten writes an annotated file once and never rewrites one in place.

/// `text` with every comment outside a string replaced by spaces (line breaks kept), so
/// a JSON parser's line and column still point into the original file.
pub fn strip_comments(text: &str) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    let mut in_string = false;
    while let Some((at, c)) = chars.next() {
        if in_string {
            out.push(c);
            match c {
                '\\' => {
                    if let Some((_, escaped)) = chars.next() {
                        out.push(escaped);
                    }
                }
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match (c, chars.peek().map(|&(_, n)| n)) {
            ('"', _) => {
                in_string = true;
                out.push(c);
            }
            ('/', Some('/')) => {
                out.push(' ');
                for (_, c) in chars.by_ref() {
                    blank(&mut out, c);
                    if c == '\n' {
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                out.push_str("  ");
                let mut closed = false;
                while let Some((_, c)) = chars.next() {
                    if c == '*' && chars.peek().map(|&(_, n)| n) == Some('/') {
                        chars.next();
                        out.push_str("  ");
                        closed = true;
                        break;
                    }
                    blank(&mut out, c);
                }
                if !closed {
                    let line = text[..at].matches('\n').count() + 1;
                    return Err(format!("the comment opened on line {line} is never closed"));
                }
            }
            _ => out.push(c),
        }
    }
    Ok(out)
}

/// A commented-out character: a line break stays, anything else becomes a space per
/// byte, so serde's byte columns still line up.
fn blank(out: &mut String, c: char) {
    if c == '\n' {
        out.push('\n');
    } else {
        out.extend(std::iter::repeat_n(' ', c.len_utf8()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> serde_json::Value {
        serde_json::from_str(&strip_comments(text).unwrap()).unwrap()
    }

    #[test]
    fn plain_json_is_unchanged() {
        let text = r#"{"a": [1, 2], "b": "x // y /* z */"}"#;
        assert_eq!(strip_comments(text).unwrap(), text);
    }

    #[test]
    fn comments_are_blanked_and_lines_kept() {
        let text = "{\n  \"a\": 1, // one\n  /* two\n  lines */ \"b\": 2\n}";
        let stripped = strip_comments(text).unwrap();
        assert_eq!(stripped.lines().count(), text.lines().count());
        assert_eq!(stripped.len(), text.len());
        assert_eq!(parse(text), serde_json::json!({"a": 1, "b": 2}));
    }

    #[test]
    fn a_wide_character_in_a_comment_keeps_the_columns() {
        let text = "{\"a\": 1, /* é */ \"b\": x} // ü";
        let stripped = strip_comments(text).unwrap();
        assert_eq!(stripped.len(), text.len());
        assert_eq!(stripped.find('x'), text.find('x'));
    }

    #[test]
    fn an_escaped_quote_does_not_end_the_string() {
        let text = r#"{"a": "say \"// not a comment\"" // a comment
}"#;
        assert_eq!(
            parse(text),
            serde_json::json!({"a": "say \"// not a comment\""})
        );
    }

    #[test]
    fn a_comment_at_the_end_without_a_newline() {
        assert_eq!(parse("{} // done"), serde_json::json!({}));
    }

    #[test]
    fn an_unclosed_block_comment_is_refused_with_its_line() {
        let err = strip_comments("{\n/* open\n}").unwrap_err();
        assert!(err.contains("line 2"), "{err}");
    }

    #[test]
    fn a_lone_slash_is_left_for_the_parser() {
        assert!(
            serde_json::from_str::<serde_json::Value>(&strip_comments("{/}").unwrap()).is_err()
        );
    }
}
