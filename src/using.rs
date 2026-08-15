//! PRINT USING format strings.
//!
//! The format is parsed here, at compile time, into a list of [`UsingPart`]s.
//! Code generation then emits a straight-line sequence of runtime calls, one
//! per part. The alternative -- interpreting the format string at run time --
//! would mean writing a format engine twice in hand-written assembly, once per
//! platform; doing it in Rust keeps the parsing logic testable and leaves the
//! runtime with just two small helpers.
//!
//! # Supported fields
//!
//! Numeric:
//!
//! | Format   | Meaning                                              |
//! |----------|------------------------------------------------------|
//! | `###`    | Digit positions; the value is right-justified         |
//! | `##.##`  | Digits either side of a decimal point                 |
//! | `+`      | Leading or trailing sign, always shown                |
//! | `-`      | Trailing sign, shown only for negatives               |
//! | `,`      | Group the integer part in thousands                   |
//! | `$$`     | Leading currency sign, floated against the digits     |
//! | `**`     | Pad with asterisks instead of spaces                  |
//! | `**$`    | Both of the above                                     |
//! | `^^^^`   | Exponential form                                      |
//!
//! String:
//!
//! | Format   | Meaning                                              |
//! |----------|------------------------------------------------------|
//! | `!`      | First character only                                  |
//! | `\   \`  | Fixed width: 2 plus the number of spaces between      |
//! | `&`      | The whole string, whatever its length                 |
//!
//! `_` escapes the next character, emitting it literally.
//!
//! A value too wide for its field is printed in full, preceded by `%`, as
//! GW-BASIC does.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

/// Flag bits passed to the runtime's numeric formatter.
pub mod flags {
    /// Group the integer part in thousands with commas.
    pub const COMMA: i64 = 1;
    /// Prefix a currency sign, floated against the leading digit.
    pub const DOLLAR: i64 = 2;
    /// Fill the field with `*` rather than spaces.
    pub const STAR: i64 = 4;
    /// Place the sign after the number rather than before.
    pub const TRAILING_SIGN: i64 = 8;
    /// Always show a sign, including `+` for positives.
    pub const FORCE_SIGN: i64 = 16;
    /// Render in exponential form.
    pub const EXPONENTIAL: i64 = 32;
}

/// How a string field decides its width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StrFieldKind {
    /// `!` -- exactly one character.
    First,
    /// `\  \` -- a fixed number of characters.
    Fixed,
    /// `&` -- the string's own length.
    Whole,
}

/// One piece of a parsed format string.
#[derive(Debug, Clone, PartialEq)]
pub enum UsingPart {
    /// Text emitted as-is.
    Literal(String),
    /// A numeric field, consuming one value.
    Num {
        /// Total field width in characters.
        width: usize,
        /// Digits after the decimal point.
        decimals: usize,
        /// Bitwise-or of [`flags`].
        flags: i64,
    },
    /// A string field, consuming one value.
    Str {
        kind: StrFieldKind,
        /// Field width; ignored for [`StrFieldKind::Whole`].
        width: usize,
    },
}

impl UsingPart {
    /// Whether this part consumes one of the printed values.
    pub fn consumes_value(&self) -> bool {
        !matches!(self, UsingPart::Literal(_))
    }
}

/// Parse a PRINT USING format string.
///
/// Unrecognized characters are treated as literal text, which is what GW-BASIC
/// does, so this never fails.
pub fn parse(format: &str) -> Vec<UsingPart> {
    let chars: Vec<char> = format.chars().collect();
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut i = 0;

    while i < chars.len() {
        // Flush pending literal text before starting a field.
        macro_rules! flush {
            () => {
                if !literal.is_empty() {
                    parts.push(UsingPart::Literal(std::mem::take(&mut literal)));
                }
            };
        }

        match chars[i] {
            // `_` escapes the next character.
            '_' if i + 1 < chars.len() => {
                literal.push(chars[i + 1]);
                i += 2;
            }

            '!' => {
                flush!();
                parts.push(UsingPart::Str {
                    kind: StrFieldKind::First,
                    width: 1,
                });
                i += 1;
            }

            '&' => {
                flush!();
                parts.push(UsingPart::Str {
                    kind: StrFieldKind::Whole,
                    width: 0,
                });
                i += 1;
            }

            // `\   \` -- width is the two backslashes plus the gap between.
            '\\' => {
                let mut j = i + 1;
                while j < chars.len() && chars[j] == ' ' {
                    j += 1;
                }
                if j < chars.len() && chars[j] == '\\' {
                    flush!();
                    parts.push(UsingPart::Str {
                        kind: StrFieldKind::Fixed,
                        width: j - i + 1,
                    });
                    i = j + 1;
                } else {
                    literal.push('\\');
                    i += 1;
                }
            }

            // A numeric field starts at #, or at one of the prefixes that
            // introduce one.
            '#' | '+' | '$' | '*' => {
                if let Some((part, next)) = parse_numeric(&chars, i) {
                    flush!();
                    parts.push(part);
                    i = next;
                } else {
                    literal.push(chars[i]);
                    i += 1;
                }
            }

            c => {
                literal.push(c);
                i += 1;
            }
        }
    }

    if !literal.is_empty() {
        parts.push(UsingPart::Literal(literal));
    }
    parts
}

/// Try to parse a numeric field starting at `start`.
///
/// Returns the field and the index just past it, or `None` when the characters
/// do not actually form one (a lone `$`, say, which is literal text).
fn parse_numeric(chars: &[char], start: usize) -> Option<(UsingPart, usize)> {
    let mut i = start;
    let mut flags = 0i64;
    let mut width = 0usize;

    // Leading sign.
    if chars[i] == '+' {
        flags |= flags::FORCE_SIGN;
        width += 1;
        i += 1;
    }

    // `**` asterisk fill, and `**$` which also floats a currency sign.
    if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
        flags |= flags::STAR;
        width += 2; // the two asterisks are themselves digit positions
        i += 2;
        if i < chars.len() && chars[i] == '$' {
            flags |= flags::DOLLAR;
            width += 1;
            i += 1;
        }
    } else if i + 1 < chars.len() && chars[i] == '$' && chars[i + 1] == '$' {
        flags |= flags::DOLLAR;
        width += 2; // one position is the sign, one holds a digit
        i += 2;
    }

    // Digit positions before the decimal point.
    let digits_start = i;
    while i < chars.len() && chars[i] == '#' {
        width += 1;
        i += 1;
    }

    // Comma grouping, written among the digits. The comma itself is not a
    // character position; instead the field grows by however many separators
    // the digits will need, so `#######,` is wide enough for "1,234,567".
    let mut int_digits = i - digits_start;
    let mut grouped = false;
    while i < chars.len() && (chars[i] == ',' || chars[i] == '#') {
        if chars[i] == ',' {
            flags |= flags::COMMA;
            grouped = true;
        } else {
            int_digits += 1;
            width += 1;
        }
        i += 1;
    }
    if grouped {
        width += int_digits.saturating_sub(1) / 3;
    }

    // Nothing here actually formed a field.
    if int_digits == 0 && width == 0 {
        return None;
    }

    // Fractional digits.
    let mut decimals = 0usize;
    if i < chars.len() && chars[i] == '.' {
        let mut j = i + 1;
        let mut d = 0;
        while j < chars.len() && chars[j] == '#' {
            d += 1;
            j += 1;
        }
        if d > 0 {
            decimals = d;
            width += 1 + d; // the point plus its digits
            i = j;
        }
    }

    // A field needs at least one digit position somewhere.
    if int_digits == 0 && decimals == 0 {
        return None;
    }

    // `^^^^` exponential form.
    if i + 3 < chars.len() && chars[i..i + 4].iter().all(|&c| c == '^') {
        flags |= flags::EXPONENTIAL;
        width += 4;
        i += 4;
    }

    // Trailing sign.
    if i < chars.len() && (chars[i] == '-' || chars[i] == '+') {
        flags |= flags::TRAILING_SIGN;
        if chars[i] == '+' {
            flags |= flags::FORCE_SIGN;
        }
        width += 1;
        i += 1;
    }

    Some((
        UsingPart::Num {
            width,
            decimals,
            flags,
        },
        i,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_literal() {
        assert_eq!(parse("hello"), vec![UsingPart::Literal("hello".into())]);
    }

    #[test]
    fn simple_numeric_field() {
        assert_eq!(
            parse("###"),
            vec![UsingPart::Num {
                width: 3,
                decimals: 0,
                flags: 0
            }]
        );
    }

    #[test]
    fn numeric_with_decimals() {
        assert_eq!(
            parse("##.##"),
            vec![UsingPart::Num {
                width: 5,
                decimals: 2,
                flags: 0
            }]
        );
    }

    #[test]
    fn literal_around_field() {
        assert_eq!(
            parse("Total: ##.## units"),
            vec![
                UsingPart::Literal("Total: ".into()),
                UsingPart::Num {
                    width: 5,
                    decimals: 2,
                    flags: 0
                },
                UsingPart::Literal(" units".into()),
            ]
        );
    }

    #[test]
    fn sign_flags() {
        let leading = parse("+###");
        assert_eq!(
            leading,
            vec![UsingPart::Num {
                width: 4,
                decimals: 0,
                flags: flags::FORCE_SIGN
            }]
        );
        let trailing = parse("###-");
        assert_eq!(
            trailing,
            vec![UsingPart::Num {
                width: 4,
                decimals: 0,
                flags: flags::TRAILING_SIGN
            }]
        );
    }

    #[test]
    fn comma_grouping() {
        // Five digit positions, plus one separator they will need, plus ".##".
        assert_eq!(
            parse("#####,.##"),
            vec![UsingPart::Num {
                width: 9,
                decimals: 2,
                flags: flags::COMMA
            }]
        );
        // Seven digits need two separators, so the field is nine wide.
        assert_eq!(
            parse("#######,"),
            vec![UsingPart::Num {
                width: 9,
                decimals: 0,
                flags: flags::COMMA
            }]
        );
    }

    #[test]
    fn currency_and_fill() {
        assert!(matches!(
            parse("$$###.##").as_slice(),
            [UsingPart::Num { flags, .. }] if flags & flags::DOLLAR != 0
        ));
        assert!(matches!(
            parse("**###").as_slice(),
            [UsingPart::Num { flags, .. }] if flags & flags::STAR != 0
        ));
        assert!(matches!(
            parse("**$###").as_slice(),
            [UsingPart::Num { flags, .. }]
                if flags & flags::STAR != 0 && flags & flags::DOLLAR != 0
        ));
    }

    #[test]
    fn exponential() {
        assert!(matches!(
            parse("##.##^^^^").as_slice(),
            [UsingPart::Num { flags, .. }] if flags & flags::EXPONENTIAL != 0
        ));
    }

    #[test]
    fn string_fields() {
        assert_eq!(
            parse("!"),
            vec![UsingPart::Str {
                kind: StrFieldKind::First,
                width: 1
            }]
        );
        assert_eq!(
            parse("&"),
            vec![UsingPart::Str {
                kind: StrFieldKind::Whole,
                width: 0
            }]
        );
        // Two backslashes with three spaces between them is a 5-wide field.
        assert_eq!(
            parse("\\   \\"),
            vec![UsingPart::Str {
                kind: StrFieldKind::Fixed,
                width: 5
            }]
        );
    }

    #[test]
    fn underscore_escapes() {
        assert_eq!(parse("_#_!"), vec![UsingPart::Literal("#!".into())]);
    }

    #[test]
    fn lone_backslash_is_literal() {
        assert_eq!(parse("a\\b"), vec![UsingPart::Literal("a\\b".into())]);
    }

    #[test]
    fn mixed_fields_consume_values() {
        let parts = parse("& has ### items");
        let consuming = parts.iter().filter(|p| p.consumes_value()).count();
        assert_eq!(consuming, 2);
    }
}
