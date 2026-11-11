use super::token::{T, Tokens, lex, quoted_end};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
    Name,
    Keyword,
    Number,
    Str { closed: bool },
    LongStr,
    Hash { closed: bool },
    LineComment,
    BlockComment,
    CComment,
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

    pub fn raw_value<'a>(&self, src: &'a str) -> &'a str {
        let (s, e) = self.content_range(src);
        &src[s..e]
    }

    pub fn string_value(&self, src: &str) -> String {
        let raw = self.raw_value(src);
        if self.kind == Tok::LongStr || !raw.contains('\\') {
            return raw.to_owned();
        }
        String::from_utf8_lossy(&decode_escapes(raw)).into_owned()
    }

    pub fn string_bytes(&self, src: &str) -> Vec<u8> {
        let raw = self.raw_value(src);
        if self.kind == Tok::LongStr { raw.as_bytes().to_vec() } else { decode_escapes(raw) }
    }
}

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

const OPS: &[&str] = &[
    "...", "<<=", ">>=", "//=", "..=", "==", "~=", "<=", ">=", "//", "::", "..", "<<", ">>", "+=", "-=", "*=", "/=",
    "%=", "&=", "|=", "^=", "!=", "?", "++", "+", "-", "*", "/", "%", "^", "#", "&", "~", "|", "<", ">", "=", "(", ")",
    "{", "}", "[", "]", ";", ":", ",", ".", "!",
];

pub fn is_keyword(s: &str) -> bool {
    KEYWORDS.contains(&s)
}

fn operators(src: &str, from: usize, to: usize, out: &mut Vec<Token>) {
    let mut i = from;
    while i < to {
        let (kind, len) = match OPS.iter().find(|op| src[i..to].starts_with(*op)) {
            Some(op) => (Tok::Op, op.len()),
            // One whole character (non-ASCII identifiers are not Lua).
            None => (Tok::Unknown, src[i..].chars().next().map_or(1, char::len_utf8)),
        };
        out.push(Token { kind, start: i, end: (i + len).min(to) });
        i += len;
    }
}

fn doc_lines(src: &str, from: usize, to: usize, out: &mut Vec<Token>) {
    let mut start = from;
    for line in src[from..to].split_inclusive('\n') {
        let end = start + line.len();
        let text = line.trim_end_matches(['\r', '\n']);
        let indent = text.len() - text.trim_start_matches([' ', '\t', '\x0c']).len();
        out.push(Token { kind: Tok::LineComment, start: start + indent, end: start + text.len() });
        start = end;
    }
}

/// The tokens the features read, white space left out, and for each token of `fine` the index of the
/// one that holds it here (of the next one for white space).
pub fn coarse(src: &str, fine: &Tokens) -> (Vec<Token>, Vec<u32>) {
    let bytes = src.as_bytes();
    let mut out: Vec<Token> = Vec::with_capacity(fine.len() / 2 + 1);
    let mut map: Vec<u32> = Vec::with_capacity(fine.len() + 1);
    let mut i = 0;
    while i < fine.len() {
        let (kind, start, end) = (fine.kind(i), fine.start(i), fine.end(i));
        map.push(out.len() as u32);
        i += 1;
        let coarse = match kind {
            T::Whitespace => continue,
            T::ShortComment | T::Region | T::EndRegion => Tok::LineComment,
            T::DocComment => {
                doc_lines(src, start, end, &mut out);
                continue;
            }
            T::BlockComment if bytes[start] == b'/' => Tok::CComment,
            T::BlockComment => Tok::BlockComment,
            T::Shebang => {
                let mut end = end;
                if i < fine.len() && fine.kind(i) == T::ShebangContent {
                    map.push(out.len() as u32);
                    end = fine.end(i);
                    i += 1;
                }
                out.push(Token { kind: Tok::LineComment, start, end });
                continue;
            }
            T::ShebangContent => Tok::LineComment,
            T::Id => Tok::Name,
            T::Number => Tok::Number,
            T::String if bytes[start] == b'[' => Tok::LongStr,
            T::String => Tok::Str { closed: quoted_end(bytes, start).1 },
            T::HashString => Tok::Hash { closed: quoted_end(bytes, start).1 },
            _ if kind.is_keyword() => Tok::Keyword,
            _ => {
                let glued = |j: usize| j < fine.len() && !fine.kind(j).is_trivia() && operator_like(fine.kind(j));
                if kind != T::BadCharacter && !glued(i) {
                    Tok::Op
                } else {
                    let first = out.len();
                    let mut last = i;
                    while glued(last) {
                        last += 1;
                    }
                    operators(src, start, fine.end(last - 1), &mut out);
                    let mut at = first;
                    for j in i..last {
                        while at + 1 < out.len() && out[at].end <= fine.start(j) {
                            at += 1;
                        }
                        map.push(at as u32);
                    }
                    i = last;
                    continue;
                }
            }
        };
        out.push(Token { kind: coarse, start, end });
    }
    map.push(out.len() as u32);
    (out, map)
}

fn operator_like(kind: T) -> bool {
    !matches!(kind, T::Id | T::Number | T::String | T::HashString | T::Shebang | T::ShebangContent)
        && !kind.is_keyword()
}

pub fn tokenize(src: &str) -> Vec<Token> {
    coarse(src, &lex(src)).0
}

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
        assert_eq!(k.iter().filter(|(t, s)| *t == Tok::Op && *s == "?").count(), 2);
    }

    #[test]
    fn question_mark_is_its_own_token() {
        assert_eq!(kinds("x ? . y"), [(Tok::Name, "x"), (Tok::Op, "?"), (Tok::Op, "."), (Tok::Name, "y")]);
        assert_eq!(kinds("x?[[s]]"), [(Tok::Name, "x"), (Tok::Op, "?"), (Tok::LongStr, "[[s]]")]);
        assert_eq!(kinds("a??b")[1..3], [(Tok::Op, "?"), (Tok::Op, "?")]);
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
