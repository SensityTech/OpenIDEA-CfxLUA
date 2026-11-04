//! A tokenizer for CfxLua: Lua 5.4 plus what the Cfx.re runtime accepts
//! (compound assignment, safe navigation, `` `hash` `` literals, C-style
//! comments). It never fails: anything it does not understand becomes an
//! [`Tok::Unknown`] token, so features keep working on half-typed code.
//!
//! Offsets are UTF-8 byte offsets into the source.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
    Name,
    Keyword,
    Number,
    /// `'…'` / `"…"`; `closed` is false when the line ends first.
    Str {
        closed: bool,
    },
    /// `[[…]]`, `[==[…]==]`.
    LongStr,
    /// `` `name` `` (compile-time joaat).
    Hash {
        closed: bool,
    },
    /// `-- …`
    LineComment,
    /// `--[[ … ]]`
    BlockComment,
    /// `/* … */` (CfxLua).
    CComment,
    /// Operators and punctuation, including the CfxLua ones (`+=`, `?.`) and
    /// the ones it does not accept but people type (`!=`, `//=`), so
    /// inspections can name them.
    Op,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: Tok,
    pub start: usize,
    pub end: usize,
}

impl Token {
    pub fn text<'a>(&self, src: &'a str) -> &'a str {
        &src[self.start..self.end]
    }

    pub fn is_comment(&self) -> bool {
        matches!(self.kind, Tok::LineComment | Tok::BlockComment | Tok::CComment)
    }

    pub fn is_string(&self) -> bool {
        matches!(self.kind, Tok::Str { .. } | Tok::LongStr)
    }

    pub fn is_op(&self, src: &str, op: &str) -> bool {
        self.kind == Tok::Op && self.text(src) == op
    }

    pub fn is_kw(&self, src: &str, kw: &str) -> bool {
        self.kind == Tok::Keyword && self.text(src) == kw
    }

    pub fn is_name(&self, src: &str, name: &str) -> bool {
        self.kind == Tok::Name && self.text(src) == name
    }

    /// Byte range of a string's content (without quotes / brackets).
    pub fn content_range(&self, src: &str) -> (usize, usize) {
        match self.kind {
            Tok::Str { closed } | Tok::Hash { closed } => {
                (self.start + 1, if closed { self.end - 1 } else { self.end }.max(self.start + 1))
            }
            Tok::LongStr => {
                let text = self.text(src);
                let open = text.find('[').and_then(|i| text[i + 1..].find('[').map(|j| i + j + 2)).unwrap_or(2);
                let level = open.saturating_sub(2);
                let close = if text.ends_with(']') { level + 2 } else { 0 };
                let mut start = self.start + open;
                // A newline right after the opening bracket is not part of the string.
                if src[start..].starts_with("\r\n") {
                    start += 2;
                } else if src[start..].starts_with('\n') {
                    start += 1;
                }
                (start.min(self.end), self.end.saturating_sub(close).max(start.min(self.end)))
            }
            _ => (self.start, self.end),
        }
    }

    /// The content as written, escapes included.
    pub fn raw_value<'a>(&self, src: &'a str) -> &'a str {
        let (s, e) = self.content_range(src);
        &src[s..e]
    }

    /// A string's value as text (invalid UTF-8 from `\xHH` / `\ddd` escapes is replaced).
    pub fn string_value(&self, src: &str) -> String {
        let raw = self.raw_value(src);
        if self.kind == Tok::LongStr || !raw.contains('\\') {
            return raw.to_owned();
        }
        String::from_utf8_lossy(&decode_escapes(raw)).into_owned()
    }

    /// A string's value as the bytes the runtime sees (Lua escapes decoded).
    pub fn string_bytes(&self, src: &str) -> Vec<u8> {
        let raw = self.raw_value(src);
        if self.kind == Tok::LongStr { raw.as_bytes().to_vec() } else { decode_escapes(raw) }
    }
}

/// Lua 5.4 quoted-string escapes: `\n` `\t` `\\` … `\xHH`, `\ddd`, `\z`, `\u{XXX}`, escaped line breaks.
pub fn decode_escapes(raw: &str) -> Vec<u8> {
    let b = raw.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' || i + 1 >= b.len() {
            out.push(b[i]);
            i += 1;
            continue;
        }
        let e = b[i + 1];
        i += 2;
        match e {
            b'a' => out.push(0x07),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(0x0b),
            b'\r' | b'\n' => {
                out.push(b'\n');
                if i < b.len() && (b[i] == b'\r' || b[i] == b'\n') && b[i] != e {
                    i += 1;
                }
            }
            b'z' => {
                while i < b.len() && b[i].is_ascii_whitespace() {
                    i += 1;
                }
            }
            b'x' => {
                let hex = raw.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(v) => {
                        out.push(v);
                        i += 2;
                    }
                    None => out.push(b'x'),
                }
            }
            b'u' if b.get(i) == Some(&b'{') => {
                let end = raw[i..].find('}').map(|n| i + n);
                match end.and_then(|end| u32::from_str_radix(&raw[i + 1..end], 16).ok().map(|v| (v, end))) {
                    Some((v, end)) => {
                        push_utf8_extended(&mut out, v);
                        i = end + 1;
                    }
                    None => out.push(b'u'),
                }
            }
            b'0'..=b'9' => {
                let mut v = u32::from(e - b'0');
                let mut n = 1;
                while n < 3 && i < b.len() && b[i].is_ascii_digit() {
                    v = v * 10 + u32::from(b[i] - b'0');
                    i += 1;
                    n += 1;
                }
                out.push(v.min(255) as u8);
            }
            other => out.push(other),
        }
    }
    out
}

/// Lua's `utf8.char` encoding, extended to 31 bits like `\u{7FFFFFFF}`.
fn push_utf8_extended(out: &mut Vec<u8>, v: u32) {
    if v < 0x80 {
        out.push(v as u8);
        return;
    }
    let (mut v, mut mfb) = (v, 0x3fu32);
    let mut tail = Vec::with_capacity(6);
    loop {
        tail.push(0x80 | (v & 0x3f) as u8);
        v >>= 6;
        mfb >>= 1;
        if v <= mfb {
            break;
        }
    }
    out.push(((!mfb << 1) | v) as u8);
    out.extend(tail.iter().rev());
}

pub const KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in", "local", "nil",
    "not", "or", "repeat", "return", "then", "true", "until", "while",
];

/// Longest first, so `>>=` wins over `>>` and `>`.
const OPS: &[&str] = &[
    "...", "<<=", ">>=", "//=", "..=", "==", "~=", "<=", ">=", "//", "::", "..", "<<", ">>", "+=", "-=", "*=", "/=",
    "%=", "&=", "|=", "^=", "!=", "?.", "?[", "?:", "++", "+", "-", "*", "/", "%", "^", "#", "&", "~", "|", "<", ">",
    "=", "(", ")", "{", "}", "[", "]", ";", ":", ",", ".", "!",
];

pub fn is_keyword(s: &str) -> bool {
    KEYWORDS.contains(&s)
}

/// Level of a long bracket opening at `at` (`[[` → 0, `[==[` → 2).
fn long_bracket(bytes: &[u8], at: usize) -> Option<usize> {
    if bytes.get(at) != Some(&b'[') {
        return None;
    }
    let mut i = at + 1;
    while bytes.get(i) == Some(&b'=') {
        i += 1;
    }
    (bytes.get(i) == Some(&b'[')).then_some(i - at - 1)
}

/// End (exclusive) of a long bracket body closing at level `level`.
fn long_end(src: &str, from: usize, level: usize) -> usize {
    let close = format!("]{}]", "=".repeat(level));
    src[from..].find(&close).map_or(src.len(), |i| from + i + close.len())
}

pub fn tokenize(src: &str) -> Vec<Token> {
    let bytes = src.as_bytes();
    let mut out = Vec::with_capacity(src.len() / 4);
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let start = i;
        let kind = match c {
            b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c => {
                i += 1;
                continue;
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                if let Some(level) = long_bracket(bytes, i + 2) {
                    i = long_end(src, i + 2 + level + 2, level);
                    Tok::BlockComment
                } else {
                    i = src[i..].find('\n').map_or(src.len(), |n| i + n);
                    Tok::LineComment
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = src[i + 2..].find("*/").map_or(src.len(), |n| i + 2 + n + 2);
                Tok::CComment
            }
            b'[' if long_bracket(bytes, i).is_some() => {
                let level = long_bracket(bytes, i).unwrap_or(0);
                i = long_end(src, i + level + 2, level);
                Tok::LongStr
            }
            b'\'' | b'"' | b'`' => {
                i += 1;
                let mut closed = false;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => {
                            i += match bytes.get(i + 1) {
                                Some(b'\r') if bytes.get(i + 2) == Some(&b'\n') => 3,
                                Some(b'z') => {
                                    // `\z` skips the whitespace after it, newlines included.
                                    let mut j = i + 2;
                                    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                                        j += 1;
                                    }
                                    j - i
                                }
                                _ => 2,
                            };
                        }
                        b'\n' => break,
                        b if b == c => {
                            i += 1;
                            closed = true;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                i = i.min(bytes.len());
                if c == b'`' { Tok::Hash { closed } } else { Tok::Str { closed } }
            }
            b'0'..=b'9' => {
                i = number_end(bytes, i);
                Tok::Number
            }
            b'.' if bytes.get(i + 1).is_some_and(u8::is_ascii_digit) => {
                i = number_end(bytes, i);
                Tok::Number
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                if is_keyword(&src[start..i]) { Tok::Keyword } else { Tok::Name }
            }
            _ => {
                if let Some(op) = OPS.iter().find(|op| src[i..].starts_with(*op)) {
                    i += op.len();
                    Tok::Op
                } else {
                    // One whole character (non-ASCII identifiers are not Lua).
                    i += src[i..].chars().next().map_or(1, char::len_utf8);
                    Tok::Unknown
                }
            }
        };
        out.push(Token { kind, start, end: i.min(src.len()) });
    }
    out
}

fn number_end(bytes: &[u8], mut i: usize) -> usize {
    let hex = bytes[i] == b'0' && matches!(bytes.get(i + 1), Some(b'x' | b'X'));
    if hex {
        i += 2;
    }
    while i < bytes.len() {
        let b = bytes[i];
        let exp = if hex { matches!(b, b'p' | b'P') } else { matches!(b, b'e' | b'E') };
        if exp && matches!(bytes.get(i + 1), Some(b'+' | b'-')) {
            i += 2;
        } else if b.is_ascii_alphanumeric() || b == b'.' || b == b'_' {
            // `1..2` is a number followed by `..`.
            if b == b'.' && bytes.get(i + 1) == Some(&b'.') {
                break;
            }
            i += 1;
        } else {
            break;
        }
    }
    i
}

/// Parses a Lua number literal as an integer (hex wraps like Lua's 64-bit
/// integers; `0xFFFFFFFF` stays positive).
pub fn parse_int(text: &str) -> Option<i64> {
    let t = text.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        if hex.is_empty() || hex.len() > 16 {
            return None;
        }
        return u64::from_str_radix(hex, 16).ok().map(|v| v as i64);
    }
    t.parse::<i64>().ok()
}

/// The tokens of a source without comments, with the index of each in the
/// full token list. Most analyses look past comments.
pub fn code_tokens(tokens: &[Token]) -> Vec<Token> {
    tokens.iter().filter(|t| !t.is_comment()).copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<(Tok, &str)> {
        tokenize(src).into_iter().map(|t| (t.kind, t.text(src))).collect()
    }

    #[test]
    fn cfx_operators_and_comments() {
        let src = "count += 1 -- add\nmask &= ~0x4 /* c */ local x = p?.state?.job";
        let k = kinds(src);
        assert!(k.contains(&(Tok::Op, "+=")));
        assert!(k.contains(&(Tok::LineComment, "-- add")));
        assert!(k.contains(&(Tok::Op, "&=")));
        assert!(k.contains(&(Tok::Number, "0x4")));
        assert!(k.contains(&(Tok::CComment, "/* c */")));
        assert_eq!(k.iter().filter(|(t, s)| *t == Tok::Op && *s == "?.").count(), 2);
    }

    #[test]
    fn strings_hashes_and_long_brackets() {
        let src = "a = `adder` .. 'it\\'s' .. [==[x]]y]==] --[[ block\n]] b";
        let k = kinds(src);
        assert!(k.contains(&(Tok::Hash { closed: true }, "`adder`")));
        assert!(k.contains(&(Tok::Str { closed: true }, "'it\\'s'")));
        assert!(k.contains(&(Tok::LongStr, "[==[x]]y]==]")));
        assert!(k.contains(&(Tok::BlockComment, "--[[ block\n]]")));
        let toks = tokenize(src);
        let s = toks.iter().find(|t| t.kind == Tok::Str { closed: true }).unwrap();
        assert_eq!(s.string_value(src), "it's");
        let l = toks.iter().find(|t| t.kind == Tok::LongStr).unwrap();
        assert_eq!(l.string_value(src), "x]]y");
    }

    #[test]
    fn escapes_inside_strings_and_hashes() {
        let src = "a = `it\\`s` b = 'x\\z\n   y' c = 1";
        let toks = tokenize(src);
        let hash = toks.iter().find(|t| matches!(t.kind, Tok::Hash { .. })).unwrap();
        assert_eq!(hash.text(src), "`it\\`s`", "an escaped backtick does not close the literal");
        let s = toks.iter().find(|t| matches!(t.kind, Tok::Str { .. })).unwrap();
        assert_eq!(s.kind, Tok::Str { closed: true });
        assert_eq!(s.string_value(src), "xy");
        assert!(toks.iter().any(|t| t.is_name(src, "c")));
        assert_eq!(decode_escapes("\\65\\x42\\u{43}\\n"), b"ABC\n");
    }

    #[test]
    fn unclosed_string_stops_at_line_end() {
        let src = "TriggerEvent('abc\nnext()";
        let toks = tokenize(src);
        assert_eq!(toks[2].kind, Tok::Str { closed: false });
        assert_eq!(toks[2].text(src), "'abc");
        assert_eq!(toks[2].string_value(src), "abc");
        assert!(toks.iter().any(|t| t.is_name(src, "next")));
    }

    #[test]
    fn numbers() {
        assert_eq!(kinds("1..2"), vec![(Tok::Number, "1"), (Tok::Op, ".."), (Tok::Number, "2")]);
        assert_eq!(kinds("1e-3 0x1p4 .5"), vec![(Tok::Number, "1e-3"), (Tok::Number, "0x1p4"), (Tok::Number, ".5")]);
        assert_eq!(parse_int("0xFFFFFFFF"), Some(0xFFFF_FFFF));
        assert_eq!(parse_int("-12"), Some(-12));
    }

    #[test]
    fn invalid_ops_are_named() {
        let k = kinds("if a != b then i++ x //= 2 s ..= 'x' end");
        assert!(k.contains(&(Tok::Op, "!=")));
        assert!(k.contains(&(Tok::Op, "++")));
        assert!(k.contains(&(Tok::Op, "//=")));
        assert!(k.contains(&(Tok::Op, "..=")));
    }
}
