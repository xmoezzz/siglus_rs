use crate::error::{Error, Result};
use crate::guid::Guid;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Template,
    Array,
    Identifier(String),
    Integer(i64),
    Float(f64),
    String(String),
    Guid(Guid),
    OpenBrace,
    CloseBrace,
    OpenBracket,
    CloseBracket,
    Comma,
    Semicolon,
    Dot,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub offset: usize,
}

pub fn lex_text(input: &[u8]) -> Result<Vec<Token>> {
    let text = String::from_utf8_lossy(input);
    let text = text.as_ref();
    let bytes = text.as_bytes();
    let mut i = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) { 3 } else { 0usize };
    let mut tokens = Vec::new();

    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'#' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i += 2;
                let mut closed = false;
                while i + 1 < bytes.len() {
                    if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        i += 2;
                        closed = true;
                        break;
                    }
                    i += 1;
                }
                if !closed {
                    return Err(Error::Lex("unterminated block comment".to_string()));
                }
            }
            b'{' => {
                tokens.push(Token { kind: TokenKind::OpenBrace, offset: i });
                i += 1;
            }
            b'}' => {
                tokens.push(Token { kind: TokenKind::CloseBrace, offset: i });
                i += 1;
            }
            b'[' => {
                tokens.push(Token { kind: TokenKind::OpenBracket, offset: i });
                i += 1;
            }
            b']' => {
                tokens.push(Token { kind: TokenKind::CloseBracket, offset: i });
                i += 1;
            }
            b',' => {
                tokens.push(Token { kind: TokenKind::Comma, offset: i });
                i += 1;
            }
            b';' => {
                tokens.push(Token { kind: TokenKind::Semicolon, offset: i });
                i += 1;
            }
            b'.' => {
                tokens.push(Token { kind: TokenKind::Dot, offset: i });
                i += 1;
            }
            b'"' => {
                let start = i;
                i += 1;
                let mut out = String::new();
                let mut closed = false;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => {
                            i += 1;
                            if i >= bytes.len() {
                                return Err(Error::Lex("unterminated escape sequence".to_string()));
                            }
                            let escaped = match bytes[i] {
                                b'"' => '"',
                                b'\\' => '\\',
                                b'n' => '\n',
                                b'r' => '\r',
                                b't' => '\t',
                                other => other as char,
                            };
                            out.push(escaped);
                            i += 1;
                        }
                        b'"' => {
                            i += 1;
                            closed = true;
                            break;
                        }
                        other => {
                            out.push(other as char);
                            i += 1;
                        }
                    }
                }
                if !closed {
                    return Err(Error::Lex("unterminated string literal".to_string()));
                }
                tokens.push(Token { kind: TokenKind::String(out), offset: start });
            }
            b'<' => {
                let start = i;
                i += 1;
                let guid_start = i;
                while i < bytes.len() && bytes[i] != b'>' {
                    i += 1;
                }
                if i >= bytes.len() {
                    return Err(Error::Lex("unterminated guid".to_string()));
                }
                let raw = &text[guid_start..i];
                i += 1;
                let guid = Guid::parse(raw)?;
                tokens.push(Token { kind: TokenKind::Guid(guid), offset: start });
            }
            b'-' | b'+' | b'0'..=b'9' => {
                let start = i;
                i += 1;
                while i < bytes.len() {
                    let c = bytes[i];
                    if c.is_ascii_digit()
                        || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-' | b'#')
                        || c.is_ascii_alphabetic()
                    {
                        i += 1;
                    } else {
                        break;
                    }
                }
                let raw = &text[start..i];
                let is_float = raw.contains('.')
                    || raw.contains('e')
                    || raw.contains('E')
                    || raw.contains('#')
                    || raw.chars().any(|c| c.is_ascii_alphabetic());
                let kind = if is_float {
                    match parse_x_float_literal(raw) {
                        Ok(value) => TokenKind::Float(value),
                        Err(err) if looks_like_exporter_identifier(raw) => TokenKind::Identifier(raw.to_string()),
                        Err(err) => return Err(err),
                    }
                } else {
                    match raw.parse::<i64>() {
                        Ok(value) => TokenKind::Integer(value),
                        Err(_) if looks_like_exporter_identifier(raw) => TokenKind::Identifier(raw.to_string()),
                        Err(_) => return Err(Error::Lex(format!("invalid integer literal: {raw}"))),
                    }
                };
                tokens.push(Token { kind, offset: start });
            }
            _ if is_ident_start(b as char) => {
                let start = i;
                i += 1;
                while i < bytes.len() && is_ident_continue(bytes[i] as char) {
                    i += 1;
                }
                let raw = &text[start..i];
                let lower = raw.to_ascii_lowercase();
                let kind = match lower.as_str() {
                    "template" => TokenKind::Template,
                    "array" => TokenKind::Array,
                    _ => TokenKind::Identifier(raw.to_string()),
                };
                tokens.push(Token { kind, offset: start });
            }
            other => {
                return Err(Error::Lex(format!(
                    "unexpected byte '{}' at offset {}",
                    other as char, i
                )));
            }
        }
    }

    Ok(tokens)
}


fn parse_x_float_literal(raw: &str) -> Result<f64> {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("#ind") || lower.contains("#qnan") || lower.contains("nan") {
        return Ok(0.0);
    }
    if lower == "inf" || lower == "+inf" || lower == "infinity" || lower == "+infinity" {
        return Ok(f64::INFINITY);
    }
    if lower == "-inf" || lower == "-infinity" {
        return Ok(f64::NEG_INFINITY);
    }
    raw.parse::<f64>()
        .map_err(|_| Error::Lex(format!("invalid float literal: {raw}")))
}

fn looks_like_exporter_identifier(raw: &str) -> bool {
    raw.chars().any(|c| {
        c == '_' || c == '$' || c == ':' || (c.is_ascii_alphabetic() && c != 'e' && c != 'E')
    })
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || matches!(c, '_' | '$' | '@') || !c.is_ascii()
}

fn is_ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(c, '_' | '-' | '$' | ':' | '.' | '@' | '+' | '~')
        || !c.is_ascii()
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_exporter_float_literals_are_zero() {
        let tokens = lex_text(b"Mesh M { 1; -1.#IND00; 1.#QNAN0;; }").unwrap();
        assert!(tokens.iter().any(|t| matches!(t.kind, TokenKind::Float(v) if v == 0.0)));
    }

    #[test]
    fn skips_block_comments() {
        let tokens = lex_text(b"/* ignored */ Frame Root {}").unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Identifier(ref s) if s == "Frame"));
    }

    #[test]
    fn unterminated_strings_are_errors() {
        assert!(lex_text(b"TextureFilename { \"missing-end }").is_err());
    }

    #[test]
    fn accepts_exporter_style_identifiers() {
        let tokens = lex_text(b"Frame Bip01.L_Foot$0 {}").unwrap();
        assert!(tokens.iter().any(|t| matches!(t.kind, TokenKind::Identifier(ref s) if s == "Bip01.L_Foot$0")));
    }

    #[test]
    fn accepts_bom_and_non_utf8_text_lossily() {
        let tokens = lex_text(b"\xEF\xBB\xBFFrame Root { TextureFilename { \"a\xFF.dds\"; } }").unwrap();
        assert!(tokens.iter().any(|t| matches!(t.kind, TokenKind::Identifier(ref s) if s == "Frame")));
    }

    #[test]
    fn accepts_identifier_that_starts_with_digit_when_exporter_did_that() {
        let tokens = lex_text(b"Frame 3dsmax_Bone$0 {}").unwrap();
        assert!(tokens.iter().any(|t| matches!(t.kind, TokenKind::Identifier(ref s) if s == "3dsmax_Bone$0")));
    }

}
