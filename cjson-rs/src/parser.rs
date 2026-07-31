//! # Recursive Descent JSON Parser — Zero Unsafe Code
//!
//! Parses a JSON byte slice (`&[u8]`) into the arena-backed AST defined in
//! [`crate::arena`]. Every structural node is allocated inside the caller's
//! [`Arena`]; the returned `u32` is the root node's index.
//!
//! ## IEEE 754 Compliance (cJSON Issue #838)
//!
//! Number parsing deliberately uses [`str::parse::<f64>()`] which invokes
//! Rust's stdlib Eisel-Lemire algorithm with big-integer fallback — the gold
//! standard for correctly-rounded double-precision conversion. **There is no
//! intermediate `f32` cast anywhere in this module.**
//!
//! ## Error Handling
//!
//! Every failure path returns [`Result::Err(ParseError)`]. This module
//! contains **zero** uses of `panic!`, `unwrap()`, or `expect()`.
//!
//! ## Safety
//!
//! `#![forbid(unsafe_code)]` is enforced at the module level.

#![forbid(unsafe_code)]

use crate::arena::{Arena, JsonValue, NodeId};
use std::fmt;

// ============================================================================
//  Constants
// ============================================================================

/// Maximum nesting depth for arrays/objects (matches `CJSON_NESTING_LIMIT`).
const MAX_NESTING_DEPTH: usize = 1000;

// ============================================================================
//  ParseError — position-aware error type
// ============================================================================

/// Describes *what* went wrong during parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseErrorKind {
    /// Input ended before a complete JSON value was read.
    UnexpectedEof,
    /// An unexpected byte was encountered at the current position.
    UnexpectedCharacter(u8),
    /// A literal (`true`, `false`, `null`) was malformed.
    InvalidLiteral,
    /// A number did not conform to the JSON number grammar.
    InvalidNumber,
    /// An unrecognised `\x` escape sequence inside a string.
    InvalidStringEscape,
    /// A `\uXXXX` escape contained non-hex digits or an invalid surrogate.
    InvalidUnicodeEscape,
    /// A string was opened with `"` but never closed.
    UnterminatedString,
    /// A string's raw bytes did not form valid UTF-8 after unescaping.
    InvalidUtf8,
    /// Extra non-whitespace content appeared after the root JSON value.
    TrailingContent,
    /// Expected `:` between an object key and its value.
    ExpectedColon,
    /// Expected a `"string"` key inside an object.
    ExpectedObjectKey,
    /// Array/object nesting exceeded [`MAX_NESTING_DEPTH`].
    DepthLimitExceeded,
}

/// A parse error together with the byte offset where it was detected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// What went wrong.
    pub kind: ParseErrorKind,
    /// Byte offset (0-indexed) in the original input.
    pub position: usize,
}

// ── Display / Error impls ───────────────────────────────────────────────

impl fmt::Display for ParseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "unexpected end of input"),
            Self::UnexpectedCharacter(b) => {
                if b.is_ascii_graphic() {
                    write!(f, "unexpected character '{}'", *b as char)
                } else {
                    write!(f, "unexpected byte 0x{b:02x}")
                }
            }
            Self::InvalidLiteral => write!(f, "invalid literal (expected true/false/null)"),
            Self::InvalidNumber => write!(f, "invalid number"),
            Self::InvalidStringEscape => write!(f, "invalid escape sequence in string"),
            Self::InvalidUnicodeEscape => write!(f, "invalid \\uXXXX unicode escape"),
            Self::UnterminatedString => write!(f, "unterminated string (missing closing '\"')"),
            Self::InvalidUtf8 => write!(f, "string contains invalid UTF-8"),
            Self::TrailingContent => write!(f, "unexpected content after JSON value"),
            Self::ExpectedColon => write!(f, "expected ':' after object key"),
            Self::ExpectedObjectKey => write!(f, "expected '\"' to begin object key"),
            Self::DepthLimitExceeded => {
                write!(f, "nesting depth exceeds limit of {MAX_NESTING_DEPTH}")
            }
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JSON parse error at byte {}: {}", self.position, self.kind)
    }
}

impl std::error::Error for ParseError {}

// ============================================================================
//  Public API
// ============================================================================

/// Parse a complete JSON document from `input` into `arena`.
///
/// Returns the **root node's index** (`u32`) on success, or a
/// [`ParseError`] describing the first error encountered.
///
/// The input must contain exactly one JSON value (plus optional surrounding
/// whitespace). Trailing non-whitespace after the root value is an error.
///
/// # Example
///
/// ```rust
/// use cjson_rs::arena::{Arena, JsonValue};
/// use cjson_rs::parser::parse_json;
///
/// let mut arena = Arena::new();
/// let root = parse_json(b"[1, 2, 3]", &mut arena).unwrap();
///
/// // `root` is a u32 index into the arena.
/// ```
pub fn parse_json(input: &[u8], arena: &mut Arena) -> Result<u32, ParseError> {
    let mut parser = Parser::new(input);
    let root_id = parser.parse_value(arena)?;

    // Reject trailing content: `42 "oops"` is not valid JSON.
    parser.skip_whitespace();
    if parser.pos < parser.input.len() {
        return Err(parser.err(ParseErrorKind::TrailingContent));
    }

    Ok(root_id.index() as u32)
}

// ============================================================================
//  Parser — internal recursive descent engine
// ============================================================================

/// Stateful cursor over the input byte slice.
struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    // ── Construction ─────────────────────────────────────────────────────

    fn new(input: &'a [u8]) -> Self {
        Parser {
            input,
            pos: 0,
            depth: 0,
        }
    }

    // ── Error helpers ────────────────────────────────────────────────────

    /// Build a `ParseError` anchored at the **current** cursor position.
    #[inline]
    fn err(&self, kind: ParseErrorKind) -> ParseError {
        ParseError {
            kind,
            position: self.pos,
        }
    }

    /// Build a `ParseError` anchored at an **explicit** position.
    #[inline]
    fn err_at(&self, kind: ParseErrorKind, position: usize) -> ParseError {
        ParseError { kind, position }
    }

    // ── Low-level cursor ops ─────────────────────────────────────────────

    /// Look at the current byte without consuming it.
    #[inline]
    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    /// Consume and return the current byte, advancing the cursor.
    #[inline]
    fn advance(&mut self) -> Option<u8> {
        let b = self.input.get(self.pos).copied()?;
        self.pos += 1;
        Some(b)
    }

    /// Skip over JSON whitespace (SP, HT, LF, CR).
    fn skip_whitespace(&mut self) {
        while let Some(&b) = self.input.get(self.pos) {
            match b {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                _ => break,
            }
        }
    }

    // ── Depth tracking ───────────────────────────────────────────────────

    /// Called when entering a `[` or `{`. Returns `Err` if too deep.
    fn enter_container(&mut self) -> Result<(), ParseError> {
        self.depth += 1;
        if self.depth > MAX_NESTING_DEPTH {
            return Err(self.err(ParseErrorKind::DepthLimitExceeded));
        }
        Ok(())
    }

    /// Called when leaving a `]` or `}`.
    fn leave_container(&mut self) {
        // Guaranteed >= 1 because every `leave` is preceded by a successful
        // `enter`.  Use `saturating_sub` to uphold the "no panic" contract.
        self.depth = self.depth.saturating_sub(1);
    }

    // ── Top-level dispatch ───────────────────────────────────────────────

    /// Parse one JSON value at the current cursor position.
    fn parse_value(&mut self, arena: &mut Arena) -> Result<NodeId, ParseError> {
        self.skip_whitespace();

        match self.peek() {
            Some(b'"') => self.parse_string_node(arena),
            Some(b'{') => self.parse_object(arena),
            Some(b'[') => self.parse_array(arena),
            Some(b't') => self.parse_literal(arena, b"true", JsonValue::Bool(true)),
            Some(b'f') => self.parse_literal(arena, b"false", JsonValue::Bool(false)),
            Some(b'n') => self.parse_literal(arena, b"null", JsonValue::Null),
            Some(b'-' | b'0'..=b'9') => self.parse_number(arena),
            Some(b) => Err(self.err(ParseErrorKind::UnexpectedCharacter(b))),
            None => Err(self.err(ParseErrorKind::UnexpectedEof)),
        }
    }

    // ── Literals ─────────────────────────────────────────────────────────

    /// Match an exact byte sequence (`true`, `false`, `null`).
    fn parse_literal(
        &mut self,
        arena: &mut Arena,
        expected: &[u8],
        value: JsonValue,
    ) -> Result<NodeId, ParseError> {
        let start = self.pos;
        for &expected_byte in expected {
            match self.advance() {
                Some(b) if b == expected_byte => {}
                _ => return Err(self.err_at(ParseErrorKind::InvalidLiteral, start)),
            }
        }
        Ok(arena.alloc(value))
    }

    // ── Numbers (IEEE 754 f64 — no f32 truncation) ──────────────────────

    /// Parse a JSON number according to the grammar:
    ///
    /// ```text
    /// number = [ "-" ] int [ frac ] [ exp ]
    /// int    = "0" | digit1-9 *digit
    /// frac   = "." 1*digit
    /// exp    = ("e" | "E") [ "+" | "-" ] 1*digit
    /// ```
    ///
    /// ## IEEE 754 Compliance
    ///
    /// The validated byte span is converted to `f64` via
    /// [`str::parse::<f64>()`] which uses the **Eisel-Lemire** algorithm
    /// with a big-integer fallback for correctly-rounded results.
    /// **No intermediate `f32` cast exists anywhere in this path.**
    fn parse_number(&mut self, arena: &mut Arena) -> Result<NodeId, ParseError> {
        let start = self.pos;

        // ── optional leading minus ──
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }

        // ── integer part ──
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
                // JSON forbids leading zeros: `01`, `00`, etc.
                if matches!(self.peek(), Some(b'0'..=b'9')) {
                    return Err(self.err_at(ParseErrorKind::InvalidNumber, start));
                }
            }
            Some(b'1'..=b'9') => {
                self.pos += 1;
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
            _ => return Err(self.err_at(ParseErrorKind::InvalidNumber, start)),
        }

        // ── optional fractional part ──
        if self.peek() == Some(b'.') {
            self.pos += 1;
            // At least one digit must follow the decimal point.
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.err_at(ParseErrorKind::InvalidNumber, start));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }

        // ── optional exponent ──
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            // Optional sign.
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            // At least one digit must follow.
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.err_at(ParseErrorKind::InvalidNumber, start));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }

        // ── convert to f64 ──
        // The scanned region contains only ASCII digits, '.', 'e', 'E',
        // '+', '-' — so `from_utf8` is infallible (but we avoid `unwrap`).
        let num_bytes = &self.input[start..self.pos];
        let num_str = std::str::from_utf8(num_bytes)
            .map_err(|_| self.err_at(ParseErrorKind::InvalidNumber, start))?;

        // ┌──────────────────────────────────────────────────────────────┐
        // │  THIS IS THE CRITICAL LINE FOR IEEE 754 COMPLIANCE.         │
        // │                                                              │
        // │  Rust's str::parse::<f64>() uses the Eisel-Lemire algorithm  │
        // │  with a big-integer fallback, producing a correctly-rounded  │
        // │  f64 for ALL valid decimal strings.                          │
        // │                                                              │
        // │  There is NO f32 intermediate.  This explicitly avoids the   │
        // │  truncation bug documented in cJSON Issue #838.              │
        // └──────────────────────────────────────────────────────────────┘
        let value: f64 = num_str
            .parse()
            .map_err(|_| self.err_at(ParseErrorKind::InvalidNumber, start))?;

        Ok(arena.alloc_number(value))
    }

    // ── Strings ──────────────────────────────────────────────────────────

    /// Allocate a string node in the arena.
    fn parse_string_node(&mut self, arena: &mut Arena) -> Result<NodeId, ParseError> {
        let s = self.parse_string()?;
        Ok(arena.alloc_string(s))
    }

    /// Parse a JSON string (consumed between `"` delimiters) and return
    /// the unescaped `String`.
    ///
    /// Handles all RFC 8259 escape sequences:
    ///   `\"  \\  \/  \b  \f  \n  \r  \t  \uXXXX` (incl. surrogate pairs)
    fn parse_string(&mut self) -> Result<String, ParseError> {
        let string_start = self.pos;

        // Consume opening `"`.
        match self.advance() {
            Some(b'"') => {}
            Some(b) => {
                return Err(self.err_at(
                    ParseErrorKind::UnexpectedCharacter(b),
                    self.pos.saturating_sub(1),
                ))
            }
            None => return Err(self.err(ParseErrorKind::UnexpectedEof)),
        }

        // Collect decoded bytes; raw UTF-8 passes through, escape
        // sequences are decoded to their UTF-8 byte representation.
        let mut buf: Vec<u8> = Vec::new();

        loop {
            match self.advance() {
                // ── end of string ──
                Some(b'"') => {
                    return String::from_utf8(buf)
                        .map_err(|_| self.err_at(ParseErrorKind::InvalidUtf8, string_start));
                }

                // ── escape sequences ──
                Some(b'\\') => {
                    let esc_pos = self.pos; // position of the char after '\'
                    match self.advance() {
                        Some(b'"') => buf.push(b'"'),
                        Some(b'\\') => buf.push(b'\\'),
                        Some(b'/') => buf.push(b'/'),
                        Some(b'b') => buf.push(0x08), // backspace
                        Some(b'f') => buf.push(0x0C), // form feed
                        Some(b'n') => buf.push(b'\n'),
                        Some(b'r') => buf.push(b'\r'),
                        Some(b't') => buf.push(b'\t'),
                        Some(b'u') => self.parse_unicode_escape(&mut buf)?,
                        Some(_) => {
                            return Err(self.err_at(
                                ParseErrorKind::InvalidStringEscape,
                                esc_pos.saturating_sub(1),
                            ))
                        }
                        None => {
                            return Err(self.err_at(
                                ParseErrorKind::UnterminatedString,
                                string_start,
                            ))
                        }
                    }
                }

                // ── unescaped control characters are illegal in JSON ──
                Some(b) if b < 0x20 => {
                    return Err(self.err_at(
                        ParseErrorKind::UnexpectedCharacter(b),
                        self.pos.saturating_sub(1),
                    ))
                }

                // ── regular byte (ASCII or UTF-8 continuation) ──
                Some(b) => buf.push(b),

                // ── premature EOF ──
                None => {
                    return Err(self.err_at(
                        ParseErrorKind::UnterminatedString,
                        string_start,
                    ))
                }
            }
        }
    }

    /// Decode a `\uXXXX` escape (the `\u` has already been consumed).
    /// Handles UTF-16 surrogate pairs (`\uD800`–`\uDBFF` + `\uDC00`–`\uDFFF`).
    fn parse_unicode_escape(&mut self, buf: &mut Vec<u8>) -> Result<(), ParseError> {
        let high = self.parse_hex4()?;

        if (0xD800..=0xDBFF).contains(&high) {
            // ── high surrogate — expect `\uXXXX` low surrogate ──
            let pair_pos = self.pos;
            match self.advance() {
                Some(b'\\') => {}
                _ => {
                    return Err(self.err_at(
                        ParseErrorKind::InvalidUnicodeEscape,
                        pair_pos,
                    ))
                }
            }
            match self.advance() {
                Some(b'u') => {}
                _ => {
                    return Err(self.err_at(
                        ParseErrorKind::InvalidUnicodeEscape,
                        pair_pos,
                    ))
                }
            }
            let low = self.parse_hex4()?;
            if !(0xDC00..=0xDFFF).contains(&low) {
                return Err(self.err_at(
                    ParseErrorKind::InvalidUnicodeEscape,
                    pair_pos,
                ));
            }
            // Combine surrogates into a scalar value.
            let cp = 0x10000_u32 + ((high as u32 - 0xD800) << 10) + (low as u32 - 0xDC00);
            let ch = char::from_u32(cp)
                .ok_or_else(|| self.err(ParseErrorKind::InvalidUnicodeEscape))?;
            let mut enc = [0u8; 4];
            let utf8 = ch.encode_utf8(&mut enc);
            buf.extend_from_slice(utf8.as_bytes());
        } else if (0xDC00..=0xDFFF).contains(&high) {
            // Lone low surrogate is always invalid.
            return Err(self.err(ParseErrorKind::InvalidUnicodeEscape));
        } else {
            // BMP code point.
            let ch = char::from_u32(high as u32)
                .ok_or_else(|| self.err(ParseErrorKind::InvalidUnicodeEscape))?;
            let mut enc = [0u8; 4];
            let utf8 = ch.encode_utf8(&mut enc);
            buf.extend_from_slice(utf8.as_bytes());
        }

        Ok(())
    }

    /// Read exactly 4 hex digits and return the `u16` value.
    fn parse_hex4(&mut self) -> Result<u16, ParseError> {
        let mut value: u16 = 0;
        for _ in 0..4 {
            let b = self.advance().ok_or_else(|| self.err(ParseErrorKind::UnexpectedEof))?;
            let digit = match b {
                b'0'..=b'9' => b - b'0',
                b'a'..=b'f' => 10 + b - b'a',
                b'A'..=b'F' => 10 + b - b'A',
                _ => {
                    return Err(self.err_at(
                        ParseErrorKind::InvalidUnicodeEscape,
                        self.pos.saturating_sub(1),
                    ))
                }
            };
            value = (value << 4) | digit as u16;
        }
        Ok(value)
    }

    // ── Arrays ───────────────────────────────────────────────────────────

    /// Parse `[ value ( "," value )* ]` or `[]`.
    fn parse_array(&mut self, arena: &mut Arena) -> Result<NodeId, ParseError> {
        self.enter_container()?;
        self.pos += 1; // consume `[`

        let array_id = arena.alloc_array();

        self.skip_whitespace();

        // ── empty array ──
        if self.peek() == Some(b']') {
            self.pos += 1;
            self.leave_container();
            return Ok(array_id);
        }

        // ── one or more elements ──
        loop {
            let element_id = self.parse_value(arena)?;
            arena.append_child(array_id, element_id);

            self.skip_whitespace();
            match self.advance() {
                Some(b',') => { /* next element */ }
                Some(b']') => {
                    self.leave_container();
                    return Ok(array_id);
                }
                Some(b) => {
                    return Err(self.err_at(
                        ParseErrorKind::UnexpectedCharacter(b),
                        self.pos.saturating_sub(1),
                    ))
                }
                None => return Err(self.err(ParseErrorKind::UnexpectedEof)),
            }
        }
    }

    // ── Objects ──────────────────────────────────────────────────────────

    /// Parse `{ "key": value ( "," "key": value )* }` or `{}`.
    fn parse_object(&mut self, arena: &mut Arena) -> Result<NodeId, ParseError> {
        self.enter_container()?;
        self.pos += 1; // consume `{`

        let obj_id = arena.alloc_object();

        self.skip_whitespace();

        // ── empty object ──
        if self.peek() == Some(b'}') {
            self.pos += 1;
            self.leave_container();
            return Ok(obj_id);
        }

        // ── one or more key-value pairs ──
        loop {
            self.skip_whitespace();

            // Key must be a JSON string.
            if self.peek() != Some(b'"') {
                return Err(self.err(ParseErrorKind::ExpectedObjectKey));
            }
            let key = self.parse_string()?;

            // Expect `:`.
            self.skip_whitespace();
            match self.advance() {
                Some(b':') => {}
                _ => {
                    return Err(self.err_at(
                        ParseErrorKind::ExpectedColon,
                        self.pos.saturating_sub(1),
                    ))
                }
            }

            // Value.
            let value_id = self.parse_value(arena)?;
            arena.append_child_with_key(obj_id, value_id, key);

            self.skip_whitespace();
            match self.advance() {
                Some(b',') => { /* next pair */ }
                Some(b'}') => {
                    self.leave_container();
                    return Ok(obj_id);
                }
                Some(b) => {
                    return Err(self.err_at(
                        ParseErrorKind::UnexpectedCharacter(b),
                        self.pos.saturating_sub(1),
                    ))
                }
                None => return Err(self.err(ParseErrorKind::UnexpectedEof)),
            }
        }
    }
}

// ============================================================================
//  Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::JsonValue;

    /// Helper: parse JSON and return a reference to the root node's value.
    fn parse_val(input: &[u8]) -> Result<(Arena, u32), ParseError> {
        let mut arena = Arena::new();
        let root = parse_json(input, &mut arena)?;
        Ok((arena, root))
    }

    // ── Literals ─────────────────────────────────────────────────────────

    #[test]
    fn parse_null() {
        let (arena, root) = parse_val(b"null").unwrap();
        assert_eq!(arena.node(NodeId::from_test(root)).value, JsonValue::Null);
    }

    #[test]
    fn parse_true() {
        let (arena, root) = parse_val(b"true").unwrap();
        assert_eq!(arena.node(NodeId::from_test(root)).value, JsonValue::Bool(true));
    }

    #[test]
    fn parse_false() {
        let (arena, root) = parse_val(b"  false  ").unwrap();
        assert_eq!(arena.node(NodeId::from_test(root)).value, JsonValue::Bool(false));
    }

    // ── Numbers ──────────────────────────────────────────────────────────

    #[test]
    fn parse_zero() {
        let (arena, root) = parse_val(b"0").unwrap();
        assert_eq!(arena.node(NodeId::from_test(root)).value, JsonValue::Number(0.0));
    }

    #[test]
    fn parse_integer() {
        let (arena, root) = parse_val(b"42").unwrap();
        assert_eq!(arena.node(NodeId::from_test(root)).value, JsonValue::Number(42.0));
    }

    #[test]
    fn parse_negative_integer() {
        let (arena, root) = parse_val(b"-17").unwrap();
        assert_eq!(arena.node(NodeId::from_test(root)).value, JsonValue::Number(-17.0));
    }

    #[test]
    fn parse_float() {
        let (arena, root) = parse_val(b"3.14159").unwrap();
        assert_eq!(arena.node(NodeId::from_test(root)).value, JsonValue::Number(3.14159));
    }

    #[test]
    fn parse_exponent() {
        let (arena, root) = parse_val(b"6.022e23").unwrap();
        assert_eq!(arena.node(NodeId::from_test(root)).value, JsonValue::Number(6.022e23));
    }

    #[test]
    fn parse_negative_exponent() {
        let (arena, root) = parse_val(b"1.6e-19").unwrap();
        assert_eq!(arena.node(NodeId::from_test(root)).value, JsonValue::Number(1.6e-19));
    }

    /// ## THE critical test for cJSON Issue #838.
    ///
    /// This value has >7 significant decimal digits, which exceeds `f32`'s
    /// precision (~7.2 digits). If any intermediate `f32` cast occurred,
    /// the low-order bits would be silently truncated.
    #[test]
    fn ieee754_no_f32_truncation() {
        // ── value that f32 CANNOT represent exactly ──
        let input = b"1.23456789012345";
        let expected: f64 = 1.23456789012345_f64;
        let f32_lossy: f64 = 1.23456789012345_f32 as f64;

        // Sanity: f32 truncation does lose precision.
        assert_ne!(
            expected, f32_lossy,
            "test is invalid: f32 should differ from f64 for this value"
        );

        let (arena, root) = parse_val(input).unwrap();
        if let JsonValue::Number(parsed) = arena.node(NodeId::from_test(root)).value {
            assert_eq!(
                parsed, expected,
                "parsed value lost precision — possible f32 truncation (Issue #838)"
            );
            assert_ne!(
                parsed, f32_lossy,
                "parsed value matches f32-truncated value — f32 intermediate detected"
            );
        } else {
            panic!("expected Number node");
        }
    }

    /// Verify that extreme f64 values (near MAX/MIN) parse correctly.
    #[test]
    fn ieee754_extreme_values() {
        // Near f64::MAX
        let (arena, root) = parse_val(b"1.7976931348623157e308").unwrap();
        if let JsonValue::Number(v) = arena.node(NodeId::from_test(root)).value {
            assert_eq!(v, f64::MAX);
        } else {
            panic!("expected Number");
        }

        // Very small positive subnormal
        let (arena, root) = parse_val(b"5e-324").unwrap();
        if let JsonValue::Number(v) = arena.node(NodeId::from_test(root)).value {
            assert!(v > 0.0, "subnormal must be positive");
            assert!(v < 1e-300, "subnormal must be tiny");
        } else {
            panic!("expected Number");
        }
    }

    // ── Strings ──────────────────────────────────────────────────────────

    #[test]
    fn parse_simple_string() {
        let (arena, root) = parse_val(b"\"hello\"").unwrap();
        assert_eq!(
            arena.node(NodeId::from_test(root)).value,
            JsonValue::String("hello".into())
        );
    }

    #[test]
    fn parse_string_with_escapes() {
        let (arena, root) = parse_val(br#""line1\nline2\ttab\\back\"quote""#).unwrap();
        assert_eq!(
            arena.node(NodeId::from_test(root)).value,
            JsonValue::String("line1\nline2\ttab\\back\"quote".into())
        );
    }

    #[test]
    fn parse_string_unicode_bmp() {
        // \u00e9 = é
        let (arena, root) = parse_val(br#""\u00e9""#).unwrap();
        assert_eq!(
            arena.node(NodeId::from_test(root)).value,
            JsonValue::String("é".into())
        );
    }

    #[test]
    fn parse_string_surrogate_pair() {
        // 🎉 = U+1F389 = \uD83C\uDF89
        let (arena, root) = parse_val(br#""\uD83C\uDF89""#).unwrap();
        assert_eq!(
            arena.node(NodeId::from_test(root)).value,
            JsonValue::String("🎉".into())
        );
    }

    #[test]
    fn parse_empty_string() {
        let (arena, root) = parse_val(b"\"\"").unwrap();
        assert_eq!(
            arena.node(NodeId::from_test(root)).value,
            JsonValue::String(String::new())
        );
    }

    // ── Arrays ───────────────────────────────────────────────────────────

    #[test]
    fn parse_empty_array() {
        let (arena, root) = parse_val(b"[]").unwrap();
        assert_eq!(arena.child_count(NodeId::from_test(root)), 0);
    }

    #[test]
    fn parse_array_of_numbers() {
        let (arena, root) = parse_val(b"[1, 2, 3]").unwrap();
        let root_id = NodeId::from_test(root);
        assert_eq!(arena.child_count(root_id), 3);

        let children: Vec<_> = arena.children(root_id).collect();
        assert_eq!(arena.node(children[0]).value, JsonValue::Number(1.0));
        assert_eq!(arena.node(children[1]).value, JsonValue::Number(2.0));
        assert_eq!(arena.node(children[2]).value, JsonValue::Number(3.0));
    }

    #[test]
    fn parse_nested_arrays() {
        let (arena, root) = parse_val(b"[[1, 2], [3]]").unwrap();
        let root_id = NodeId::from_test(root);
        assert_eq!(arena.child_count(root_id), 2);
    }

    // ── Objects ──────────────────────────────────────────────────────────

    #[test]
    fn parse_empty_object() {
        let (arena, root) = parse_val(b"{}").unwrap();
        assert_eq!(arena.child_count(NodeId::from_test(root)), 0);
    }

    #[test]
    fn parse_object_with_members() {
        let input = br#"{"name": "cJSON", "version": 1.7, "safe": true}"#;
        let (arena, root) = parse_val(input).unwrap();
        let root_id = NodeId::from_test(root);
        assert_eq!(arena.child_count(root_id), 3);

        let name_id = arena.get_object_member(root_id, "name").unwrap();
        assert_eq!(
            arena.node(name_id).value,
            JsonValue::String("cJSON".into())
        );

        let ver_id = arena.get_object_member(root_id, "version").unwrap();
        assert_eq!(arena.node(ver_id).value, JsonValue::Number(1.7));

        let safe_id = arena.get_object_member(root_id, "safe").unwrap();
        assert_eq!(arena.node(safe_id).value, JsonValue::Bool(true));
    }

    // ── Complex nested document ──────────────────────────────────────────

    #[test]
    fn parse_complex_document() {
        let input = br#"
        {
            "library": "cJSON",
            "version": 1.7,
            "features": ["parsing", "printing", "manipulation"],
            "metadata": {
                "license": "MIT",
                "stars": 11000,
                "active": true,
                "deprecated": null
            }
        }
        "#;
        let (arena, root) = parse_val(input).unwrap();
        let root_id = NodeId::from_test(root);
        assert_eq!(arena.child_count(root_id), 4);

        // Drill into nested object.
        let meta_id = arena.get_object_member(root_id, "metadata").unwrap();
        assert_eq!(arena.child_count(meta_id), 4);
        let lic_id = arena.get_object_member(meta_id, "license").unwrap();
        assert_eq!(
            arena.node(lic_id).value,
            JsonValue::String("MIT".into())
        );
    }

    // ── Error cases ──────────────────────────────────────────────────────

    #[test]
    fn err_empty_input() {
        let result = parse_json(b"", &mut Arena::new());
        assert_eq!(result.unwrap_err().kind, ParseErrorKind::UnexpectedEof);
    }

    #[test]
    fn err_whitespace_only() {
        let result = parse_json(b"   ", &mut Arena::new());
        assert_eq!(result.unwrap_err().kind, ParseErrorKind::UnexpectedEof);
    }

    #[test]
    fn err_trailing_content() {
        let result = parse_json(b"42 true", &mut Arena::new());
        assert_eq!(result.unwrap_err().kind, ParseErrorKind::TrailingContent);
    }

    #[test]
    fn err_unclosed_array() {
        let result = parse_json(b"[1, 2", &mut Arena::new());
        assert_eq!(result.unwrap_err().kind, ParseErrorKind::UnexpectedEof);
    }

    #[test]
    fn err_unclosed_object() {
        let result = parse_json(br#"{"a": 1"#, &mut Arena::new());
        assert_eq!(result.unwrap_err().kind, ParseErrorKind::UnexpectedEof);
    }

    #[test]
    fn err_missing_comma_in_array() {
        let result = parse_json(b"[1 2]", &mut Arena::new());
        assert_eq!(
            result.unwrap_err().kind,
            ParseErrorKind::UnexpectedCharacter(b'2')
        );
    }

    #[test]
    fn err_missing_colon_in_object() {
        let result = parse_json(br#"{"a" 1}"#, &mut Arena::new());
        assert_eq!(result.unwrap_err().kind, ParseErrorKind::ExpectedColon);
    }

    #[test]
    fn err_trailing_comma_in_array() {
        let result = parse_json(b"[1, 2, ]", &mut Arena::new());
        // After the comma, parser expects a value but finds `]`.
        assert_eq!(
            result.unwrap_err().kind,
            ParseErrorKind::UnexpectedCharacter(b']')
        );
    }

    #[test]
    fn err_trailing_comma_in_object() {
        let result = parse_json(br#"{"a": 1, }"#, &mut Arena::new());
        assert_eq!(result.unwrap_err().kind, ParseErrorKind::ExpectedObjectKey);
    }

    #[test]
    fn err_leading_zeros() {
        let result = parse_json(b"01", &mut Arena::new());
        assert_eq!(result.unwrap_err().kind, ParseErrorKind::InvalidNumber);
    }

    #[test]
    fn err_lone_minus() {
        let result = parse_json(b"-", &mut Arena::new());
        assert_eq!(result.unwrap_err().kind, ParseErrorKind::InvalidNumber);
    }

    #[test]
    fn err_truncated_literal() {
        let result = parse_json(b"tru", &mut Arena::new());
        assert_eq!(result.unwrap_err().kind, ParseErrorKind::InvalidLiteral);
    }

    #[test]
    fn err_unterminated_string() {
        let result = parse_json(b"\"hello", &mut Arena::new());
        assert_eq!(result.unwrap_err().kind, ParseErrorKind::UnterminatedString);
    }

    #[test]
    fn err_invalid_escape() {
        let result = parse_json(br#""\q""#, &mut Arena::new());
        assert_eq!(result.unwrap_err().kind, ParseErrorKind::InvalidStringEscape);
    }

    #[test]
    fn err_lone_low_surrogate() {
        let result = parse_json(br#""\uDC00""#, &mut Arena::new());
        assert_eq!(result.unwrap_err().kind, ParseErrorKind::InvalidUnicodeEscape);
    }

    #[test]
    fn err_high_surrogate_without_low() {
        let result = parse_json(br#""\uD800""#, &mut Arena::new());
        assert_eq!(result.unwrap_err().kind, ParseErrorKind::InvalidUnicodeEscape);
    }

    #[test]
    fn err_has_position_info() {
        // `x` is at byte 4 in `[1, x]`.
        let err = parse_json(b"[1, x]", &mut Arena::new()).unwrap_err();
        assert_eq!(err.position, 4);
        // Verify Display works without panicking.
        let msg = format!("{err}");
        assert!(msg.contains("byte 4"), "error message: {msg}");
    }
}
