//! A tolerant tokenizer of JavaScript and TypeScript (JSX included): comments
//! are skipped; strings, template literals and regular expressions are single
//! tokens. It never fails: broken code gives approximate tokens. Offsets are
//! byte offsets of the analysed text; only `[from, to)` is read.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ident,
    Str,
    Template,
    Number,
    Punct,
    Regex,
}

#[derive(Clone, Debug)]
pub struct Tok {
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
    /// A line break separates it from the previous token.
    pub newline: bool,
    /// Strings and templates.
    pub lit: Option<Box<Lit>>,
}

#[derive(Clone, Debug)]
pub enum Lit {
    /// `value`: decoded, `None` when not terminated on its line; `plain`: written without escapes.
    Str {
        value: Option<String>,
        plain: bool,
    },
    Template(Template),
}

/// A template literal: one more literal part than `${}` expressions.
#[derive(Clone, Debug)]
pub struct Template {
    pub quasis: Vec<Quasi>,
    pub exprs: Vec<Vec<Tok>>,
    pub terminated: bool,
}

/// A literal part of a template: its text (escapes decoded) and where it starts.
#[derive(Clone, Debug)]
pub struct Quasi {
    pub text: String,
    pub start: usize,
    pub plain: bool,
}

impl Tok {
    pub fn text<'a>(&self, src: &'a str) -> &'a str {
        if self.kind == Kind::Template { "`" } else { src.get(self.start..self.end).unwrap_or("") }
    }

    pub fn is_punct(&self, src: &str, p: &str) -> bool {
        self.kind == Kind::Punct && self.text(src) == p
    }

    pub fn is_ident(&self, src: &str, name: &str) -> bool {
        self.kind == Kind::Ident && self.text(src) == name
    }

    pub fn template(&self) -> Option<&Template> {
        match self.lit.as_deref() {
            Some(Lit::Template(t)) => Some(t),
            _ => None,
        }
    }

    /// A string's decoded value (`None` when not terminated).
    pub fn value(&self) -> Option<&str> {
        match self.lit.as_deref() {
            Some(Lit::Str { value, .. }) => value.as_deref(),
            _ => None,
        }
    }

    /// A string literal, or a template literal without `${}`: a constant string.
    pub fn constant(&self) -> Option<&str> {
        match self.lit.as_deref()? {
            Lit::Str { value, .. } => value.as_deref(),
            Lit::Template(t) => {
                let q = t.quasis.first()?;
                (t.exprs.is_empty() && t.terminated && q.plain).then_some(q.text.as_str())
            }
        }
    }

    /// The [`Tok::constant`] maps character by character to the source.
    pub fn constant_is_plain(&self) -> bool {
        match self.lit.as_deref() {
            Some(Lit::Str { plain, .. }) => *plain,
            Some(Lit::Template(t)) => t.quasis.first().is_some_and(|q| q.plain),
            None => false,
        }
    }

    /// Offset of the first character of the constant, inside the quotes.
    pub fn content_start(&self) -> usize {
        self.start + 1
    }
}

/// Bound of the tokens of one range: generated bundles are not worth more.
const MAX_TOKENS: usize = 400_000;

/// Multi-character punctuators, longest first. `>` is never merged with a
/// following `>`: TypeScript generics close with `>>`.
const PUNCTUATORS: &[&str] = &[
    "...", "===", "!==", "**=", "<<=", "&&=", "||=", "??=", "=>", "==", "!=", "<=", ">=", "&&", "||", "??", "?.", "++",
    "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "**", "<<",
];

/// Keywords after which `/` starts a regular expression.
const BEFORE_EXPRESSION: &[&str] = &[
    "return",
    "typeof",
    "instanceof",
    "in",
    "of",
    "new",
    "delete",
    "void",
    "throw",
    "case",
    "do",
    "else",
    "yield",
    "await",
];

pub fn tokenize(text: &str, from: usize, to: usize) -> Vec<Tok> {
    let to = to.min(text.len());
    let mut lx = Lexer { text, pos: from.min(to), to, newline: false, count: 0 };
    lx.lex(false)
}

fn ident_start(c: char) -> bool {
    c == '_' || c == '$' || c.is_alphabetic()
}

fn ident_part(c: char) -> bool {
    c == '_' || c == '$' || c.is_alphanumeric()
}

struct Lexer<'a> {
    text: &'a str,
    pos: usize,
    to: usize,
    newline: bool,
    count: usize,
}

impl Lexer<'_> {
    fn peek(&self) -> Option<char> {
        self.text.get(self.pos..self.to)?.chars().next()
    }

    fn at(&self, i: usize) -> Option<u8> {
        (i < self.to).then(|| self.text.as_bytes()[i])
    }

    /// Tokens to the end, or (`stop_at_brace`) to the `}` closing a template expression (consumed).
    fn lex(&mut self, stop_at_brace: bool) -> Vec<Tok> {
        let mut tokens: Vec<Tok> = Vec::new();
        let mut depth = 0usize;
        loop {
            self.trivia();
            let Some(c) = self.peek() else { return tokens };
            if stop_at_brace {
                if c == '{' {
                    depth += 1;
                }
                if c == '}' {
                    if depth == 0 {
                        self.pos += 1;
                        return tokens;
                    }
                    depth -= 1;
                }
            }
            let t = self.next(tokens.last());
            tokens.push(t);
            self.count += 1;
            if self.count > MAX_TOKENS {
                self.pos = self.to;
                return tokens;
            }
        }
    }

    fn trivia(&mut self) {
        self.newline = false;
        while let Some(c) = self.peek() {
            match c {
                '\n' | '\r' => {
                    self.newline = true;
                    self.pos += 1;
                }
                '/' if self.at(self.pos + 1) == Some(b'/') => {
                    while let Some(b) = self.at(self.pos) {
                        if b == b'\n' || b == b'\r' {
                            break;
                        }
                        self.pos += 1;
                    }
                }
                '/' if self.at(self.pos + 1) == Some(b'*') => {
                    let body = &self.text[self.pos + 2..self.to];
                    let stop = body.find("*/").map_or(self.to, |i| self.pos + 2 + i + 2);
                    if self.text[self.pos..stop].contains('\n') {
                        self.newline = true;
                    }
                    self.pos = stop;
                }
                c if c.is_whitespace() => self.pos += c.len_utf8(),
                _ => return,
            }
        }
    }

    fn next(&mut self, previous: Option<&Tok>) -> Tok {
        let start = self.pos;
        let nl = self.newline;
        let c = self.peek().unwrap_or(' ');
        let simple = |kind, end| Tok { kind, start, end, newline: nl, lit: None };
        if ident_start(c) {
            self.pos += c.len_utf8();
            while let Some(d) = self.peek().filter(|d| ident_part(*d)) {
                self.pos += d.len_utf8();
            }
            return simple(Kind::Ident, self.pos);
        }
        let digit_after = || self.at(self.pos + 1).is_some_and(|b| b.is_ascii_digit());
        if c.is_ascii_digit() || (c == '.' && digit_after()) {
            let hex = c == '0' && matches!(self.at(self.pos + 1), Some(b'x' | b'X'));
            self.pos += 1;
            while let Some(d) = self.peek() {
                let prev = self.text.as_bytes()[self.pos - 1];
                if d.is_alphanumeric() || d == '_' || d == '.' {
                    self.pos += d.len_utf8();
                } else if (d == '+' || d == '-') && matches!(prev, b'e' | b'E') && !hex {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            return simple(Kind::Number, self.pos);
        }
        match c {
            '\'' | '"' => self.string(c, start, nl),
            '`' => self.template(start, nl),
            '/' if regex_allowed(self.text, previous) => self.regex(start, nl).unwrap_or_else(|| self.punct(start, nl)),
            _ => self.punct(start, nl),
        }
    }

    fn string(&mut self, quote: char, start: usize, nl: bool) -> Tok {
        self.pos += 1;
        let mut value = String::new();
        let mut plain = true;
        let mut terminated = false;
        while let Some(c) = self.peek() {
            if c == quote {
                self.pos += 1;
                terminated = true;
                break;
            }
            if c == '\n' || c == '\r' {
                break;
            }
            if c == '\\' {
                plain = false;
                self.pos += 1;
                self.escape(&mut value);
                continue;
            }
            value.push(c);
            self.pos += c.len_utf8();
        }
        Tok {
            kind: Kind::Str,
            start,
            end: self.pos,
            newline: nl,
            lit: Some(Box::new(Lit::Str { value: terminated.then_some(value), plain: plain && terminated })),
        }
    }

    /// Decodes the escape whose character after `\` is at `pos`; `pos` ends after it.
    fn escape(&mut self, value: &mut String) {
        let Some(e) = self.peek() else { return };
        self.pos += e.len_utf8();
        match e {
            'n' => value.push('\n'),
            't' => value.push('\t'),
            'r' => value.push('\r'),
            'b' => value.push('\u{8}'),
            'f' => value.push('\u{c}'),
            'v' => value.push('\u{b}'),
            '0' => value.push('\0'),
            '\r' => {
                if self.at(self.pos) == Some(b'\n') {
                    self.pos += 1;
                }
            }
            '\n' | '\u{2028}' | '\u{2029}' => {}
            'x' => {
                if !self.hex(value, 2) {
                    value.push('x');
                }
            }
            'u' => {
                if self.at(self.pos) == Some(b'{') {
                    let rest = &self.text[self.pos..self.to];
                    let code = rest.find('}').filter(|&c| c > 1).and_then(|c| {
                        let v = u32::from_str_radix(&rest[1..c], 16).ok()?;
                        Some((char::from_u32(v)?, c))
                    });
                    match code {
                        Some((ch, c)) => {
                            value.push(ch);
                            self.pos += c + 1;
                        }
                        None => value.push('u'),
                    }
                } else if !self.hex(value, 4) {
                    value.push('u');
                }
            }
            other => value.push(other),
        }
    }

    /// Appends the character of the `digits` hexadecimal digits at `pos`.
    fn hex(&mut self, value: &mut String, digits: usize) -> bool {
        let Some(s) = self.text.get(self.pos..self.pos + digits).filter(|_| self.pos + digits <= self.to) else {
            return false;
        };
        match u32::from_str_radix(s, 16).ok().filter(|_| s.bytes().all(|b| b.is_ascii_hexdigit())) {
            Some(code) => {
                // Lone surrogates (`\uD83D`) have no `char`: the replacement character stands for them.
                value.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                self.pos += digits;
                true
            }
            None => false,
        }
    }

    fn template(&mut self, start: usize, nl: bool) -> Tok {
        self.pos += 1;
        let mut quasis = Vec::new();
        let mut exprs = Vec::new();
        let mut quasi_start = self.pos;
        let mut quasi = String::new();
        let mut plain = true;
        let mut terminated = false;
        while let Some(c) = self.peek() {
            if c == '`' {
                quasis.push(Quasi { text: std::mem::take(&mut quasi), start: quasi_start, plain });
                self.pos += 1;
                terminated = true;
                break;
            }
            if c == '\\' {
                plain = false;
                self.pos += 1;
                self.escape(&mut quasi);
                continue;
            }
            if c == '$' && self.at(self.pos + 1) == Some(b'{') {
                quasis.push(Quasi { text: std::mem::take(&mut quasi), start: quasi_start, plain });
                plain = true;
                self.pos += 2;
                exprs.push(self.lex(true));
                quasi_start = self.pos;
                continue;
            }
            quasi.push(c);
            self.pos += c.len_utf8();
        }
        if !terminated {
            quasis.push(Quasi { text: quasi, start: quasi_start, plain });
        }
        Tok {
            kind: Kind::Template,
            start,
            end: self.pos,
            newline: nl,
            lit: Some(Box::new(Lit::Template(Template { quasis, exprs, terminated }))),
        }
    }

    fn regex(&mut self, start: usize, nl: bool) -> Option<Tok> {
        let mut i = self.pos + 1;
        let mut class = false;
        loop {
            let b = self.at(i)?;
            match b {
                b'\n' | b'\r' => return None,
                b'\\' => {
                    i += 2;
                    continue;
                }
                b'[' => class = true,
                b']' => class = false,
                b'/' if !class => break,
                _ => {}
            }
            i += 1;
        }
        i += 1;
        self.pos = i.min(self.to);
        while let Some(d) = self.peek().filter(|d| ident_part(*d)) {
            self.pos += d.len_utf8();
        }
        Some(Tok { kind: Kind::Regex, start, end: self.pos, newline: nl, lit: None })
    }

    fn punct(&mut self, start: usize, nl: bool) -> Tok {
        let rest = &self.text[self.pos..self.to];
        for p in PUNCTUATORS {
            if rest.starts_with(p) {
                // `?.5` is a conditional followed by a number.
                if *p == "?." && rest.as_bytes().get(2).is_some_and(u8::is_ascii_digit) {
                    continue;
                }
                self.pos += p.len();
                return Tok { kind: Kind::Punct, start, end: self.pos, newline: nl, lit: None };
            }
        }
        self.pos += rest.chars().next().map_or(1, char::len_utf8);
        Tok { kind: Kind::Punct, start, end: self.pos, newline: nl, lit: None }
    }
}

fn regex_allowed(text: &str, previous: Option<&Tok>) -> bool {
    match previous {
        None => true,
        Some(p) => match p.kind {
            Kind::Ident => BEFORE_EXPRESSION.contains(&p.text(text)),
            Kind::Punct => !matches!(p.text(text), ")" | "]" | "}"),
            _ => false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<(Kind, String)> {
        tokenize(src, 0, src.len()).iter().map(|t| (t.kind, t.text(src).to_owned())).collect()
    }

    #[test]
    fn punctuation_numbers_and_regexes() {
        let src = "a?.b ?? c === d >> 1e-3 + 0x1e-1; x = /a[/]b/g; y = a / b / c; z?.5:1";
        let k = toks(src);
        for p in ["?.", "??", "===", "/"] {
            assert!(k.contains(&(Kind::Punct, p.into())), "{p}");
        }
        assert_eq!(k.iter().filter(|(_, t)| t == ">").count(), 2, ">> is never merged");
        assert!(k.contains(&(Kind::Number, "1e-3".into())));
        assert!(k.contains(&(Kind::Number, "0x1e".into())), "no exponent sign in hex");
        assert!(k.contains(&(Kind::Regex, "/a[/]b/g".into())));
        assert!(k.contains(&(Kind::Number, ".5".into())), "?. before a digit");
    }

    #[test]
    fn strings_and_templates() {
        let src = "'it\\'s \\x41\\u0042\\u{43}' \"plain\" 'open\nnext `a ${b + `c${d}`} e`";
        let t = tokenize(src, 0, src.len());
        assert_eq!(t[0].value(), Some("it's ABC"));
        assert!(!t[0].constant_is_plain());
        assert_eq!(t[1].constant(), Some("plain"));
        assert!(t[1].constant_is_plain());
        assert_eq!(t[2].value(), None, "unterminated");
        assert_eq!(t[3].text(src), "next");
        let tpl = t[4].template().unwrap();
        assert_eq!(tpl.quasis.iter().map(|q| q.text.as_str()).collect::<Vec<_>>(), ["a ", " e"]);
        assert_eq!(tpl.exprs[0].len(), 3, "b + a nested template");
        assert!(t[4].constant().is_none());
        assert_eq!(t.len(), 5);
        let only = "`close`";
        assert_eq!(tokenize(only, 0, only.len())[0].constant(), Some("close"));
    }

    #[test]
    fn comments_and_ranges() {
        let src = "<script>// x\n/* y\n */ go()</script>";
        let t = tokenize(src, 8, src.len() - 9);
        assert_eq!(t.iter().map(|t| t.text(src)).collect::<Vec<_>>(), ["go", "(", ")"]);
        assert!(t[0].newline);
    }
}
