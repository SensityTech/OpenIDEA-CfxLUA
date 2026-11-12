#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum T {
    Whitespace,
    ShortComment,
    DocComment,
    BlockComment,
    Region,
    EndRegion,
    Shebang,
    ShebangContent,
    Id,
    Number,
    String,
    HashString,
    And,
    Break,
    Do,
    Else,
    ElseIf,
    End,
    False,
    For,
    Function,
    Goto,
    If,
    In,
    Local,
    Nil,
    Not,
    Or,
    Repeat,
    Return,
    Then,
    True,
    Until,
    While,
    Ellipsis,
    Concat,
    Eq,
    Ge,
    BitRtRt,
    Le,
    BitLtLt,
    Ne,
    BitTilde,
    Minus,
    Plus,
    Mult,
    Mod,
    DoubleDiv,
    Div,
    PlusAssign,
    MinusAssign,
    MultAssign,
    DivAssign,
    BitLtLtAssign,
    BitRtRtAssign,
    BitAndAssign,
    BitOrAssign,
    BitXorAssign,
    Question,
    Assign,
    Gt,
    Lt,
    LParen,
    RParen,
    LBrack,
    RBrack,
    LCurly,
    RCurly,
    GetN,
    Comma,
    Semi,
    DoubleColon,
    Colon,
    Dot,
    Exp,
    BitAnd,
    BitOr,
    BadCharacter,
    DocDashes,
    DocAt,
    DocTagName,
    DocTagField,
    DocTagParam,
    DocTagVararg,
    DocTagClass,
    DocTagEnum,
    DocTagModule,
    DocTagReturn,
    DocTagType,
    DocTagOverload,
    DocTagCast,
    DocTagPrivate,
    DocTagProtected,
    DocTagPublic,
    DocTagLanguage,
    DocTagGeneric,
    DocTagSee,
    DocTagAlias,
    DocTagSuppress,
    DocArr,
    DocFun,
    DocVararg,
    DocPrivate,
    DocProtected,
    DocPublic,
    DocStringBegin,
    DocString,
    DocAliasBar,
}

const KEYWORDS: &[(&str, T)] = &[
    ("and", T::And),
    ("break", T::Break),
    ("do", T::Do),
    ("else", T::Else),
    ("elseif", T::ElseIf),
    ("end", T::End),
    ("false", T::False),
    ("for", T::For),
    ("function", T::Function),
    ("goto", T::Goto),
    ("if", T::If),
    ("in", T::In),
    ("local", T::Local),
    ("nil", T::Nil),
    ("not", T::Not),
    ("or", T::Or),
    ("repeat", T::Repeat),
    ("return", T::Return),
    ("then", T::Then),
    ("true", T::True),
    ("until", T::Until),
    ("while", T::While),
];

const OPERATORS: &[(&str, T)] = &[
    ("...", T::Ellipsis),
    ("..", T::Concat),
    ("==", T::Eq),
    (">=", T::Ge),
    (">>", T::BitRtRt),
    ("<=", T::Le),
    ("<<", T::BitLtLt),
    ("~=", T::Ne),
    ("~", T::BitTilde),
    ("-", T::Minus),
    ("+", T::Plus),
    ("*", T::Mult),
    ("%", T::Mod),
    ("//", T::DoubleDiv),
    ("/", T::Div),
    ("+=", T::PlusAssign),
    ("-=", T::MinusAssign),
    ("*=", T::MultAssign),
    ("/=", T::DivAssign),
    ("<<=", T::BitLtLtAssign),
    (">>=", T::BitRtRtAssign),
    ("&=", T::BitAndAssign),
    ("|=", T::BitOrAssign),
    ("^=", T::BitXorAssign),
    ("?", T::Question),
    ("=", T::Assign),
    (">", T::Gt),
    ("<", T::Lt),
    ("(", T::LParen),
    (")", T::RParen),
    ("[", T::LBrack),
    ("]", T::RBrack),
    ("{", T::LCurly),
    ("}", T::RCurly),
    ("#", T::GetN),
    (",", T::Comma),
    (";", T::Semi),
    ("::", T::DoubleColon),
    (":", T::Colon),
    (".", T::Dot),
    ("^", T::Exp),
    ("&", T::BitAnd),
    ("|", T::BitOr),
];

fn operator(bytes: &[u8], i: usize) -> Option<(T, usize)> {
    let second = bytes.get(i + 1).copied();
    let third = bytes.get(i + 2).copied();
    let with_assign = |plain: T, assign: T| if second == Some(b'=') { (assign, 2) } else { (plain, 1) };
    Some(match bytes[i] {
        b'.' if second == Some(b'.') => {
            if third == Some(b'.') {
                (T::Ellipsis, 3)
            } else {
                (T::Concat, 2)
            }
        }
        b'.' => (T::Dot, 1),
        b'=' => with_assign(T::Assign, T::Eq),
        b'~' => with_assign(T::BitTilde, T::Ne),
        b'+' => with_assign(T::Plus, T::PlusAssign),
        b'-' => with_assign(T::Minus, T::MinusAssign),
        b'*' => with_assign(T::Mult, T::MultAssign),
        b'&' => with_assign(T::BitAnd, T::BitAndAssign),
        b'|' => with_assign(T::BitOr, T::BitOrAssign),
        b'^' => with_assign(T::Exp, T::BitXorAssign),
        b'/' if second == Some(b'/') => (T::DoubleDiv, 2),
        b'/' => with_assign(T::Div, T::DivAssign),
        b'<' if second == Some(b'<') => {
            if third == Some(b'=') {
                (T::BitLtLtAssign, 3)
            } else {
                (T::BitLtLt, 2)
            }
        }
        b'<' => with_assign(T::Lt, T::Le),
        b'>' if second == Some(b'>') => {
            if third == Some(b'=') {
                (T::BitRtRtAssign, 3)
            } else {
                (T::BitRtRt, 2)
            }
        }
        b'>' => with_assign(T::Gt, T::Ge),
        b':' if second == Some(b':') => (T::DoubleColon, 2),
        b':' => (T::Colon, 1),
        b'%' => (T::Mod, 1),
        b'?' => (T::Question, 1),
        b'(' => (T::LParen, 1),
        b')' => (T::RParen, 1),
        b'[' => (T::LBrack, 1),
        b']' => (T::RBrack, 1),
        b'{' => (T::LCurly, 1),
        b'}' => (T::RCurly, 1),
        b'#' => (T::GetN, 1),
        b',' => (T::Comma, 1),
        b';' => (T::Semi, 1),
        _ => return None,
    })
}

impl T {
    pub fn is_comment(self) -> bool {
        matches!(self, T::ShortComment | T::DocComment | T::BlockComment | T::Region | T::EndRegion)
    }

    pub fn is_trivia(self) -> bool {
        self == T::Whitespace || self.is_comment()
    }

    pub fn is_keyword(self) -> bool {
        (T::And as u8..=T::While as u8).contains(&(self as u8))
    }

    /// The names of the documentation tags the JetBrains plugin colours: every one but `suppress`.
    pub fn is_doc_tag(self) -> bool {
        (T::DocTagName as u8..=T::DocTagAlias as u8).contains(&(self as u8))
    }

    pub fn is_compound_assign(self) -> bool {
        (T::PlusAssign as u8..=T::BitXorAssign as u8).contains(&(self as u8))
    }

    pub fn keyword(text: &str) -> Option<T> {
        if !(2..=8).contains(&text.len()) {
            return None;
        }
        KEYWORDS.iter().find(|(word, _)| *word == text).map(|(_, kind)| *kind)
    }

    /// The name IntelliJ prints for this token type: its text for keywords and operators.
    pub fn name(self) -> &'static str {
        match self {
            T::Whitespace => "WHITE_SPACE",
            T::ShortComment => "SHORT_COMMENT",
            T::DocComment => "DOC_COMMENT",
            T::BlockComment => "BLOCK_COMMENT",
            T::Region => "REGION",
            T::EndRegion => "ENDREGION",
            T::Shebang => "#!",
            T::ShebangContent => "SHEBANG_CONTENT",
            T::Id => "ID",
            T::Number => "NUMBER",
            T::String => "STRING",
            T::HashString => "HASH_STRING",
            T::BadCharacter => "BAD_CHARACTER",
            T::DocDashes => "DASHES",
            T::DocAt => "@",
            T::DocTagName => "TAG_NAME",
            T::DocTagField => "TAG_NAME_FIELD",
            T::DocTagParam => "TAG_NAME_PARAM",
            T::DocTagVararg => "TAG_NAME_VARARG",
            T::DocTagClass => "TAG_NAME_CLASS",
            T::DocTagEnum => "TAG_NAME_ENUM",
            T::DocTagModule => "TAG_NAME_MODULE",
            T::DocTagReturn => "TAG_NAME_RETURN",
            T::DocTagType => "TAG_NAME_TYPE",
            T::DocTagOverload => "TAG_NAME_OVERLOAD",
            T::DocTagCast => "TAG_NAME_CAST",
            T::DocTagPrivate => "TAG_NAME_PRIVATE",
            T::DocTagProtected => "TAG_NAME_PROTECTED",
            T::DocTagPublic => "TAG_NAME_PUBLIC",
            T::DocTagLanguage => "TAG_NAME_LANGUAGE",
            T::DocTagGeneric => "TAG_NAME_GENERIC",
            T::DocTagSee => "TAG_NAME_SEE",
            T::DocTagAlias => "TAG_NAME_ALIAS",
            T::DocTagSuppress => "TAG_NAME_SUPPRESS",
            T::DocArr => "[]",
            T::DocFun => "fun",
            T::DocVararg => "vararg",
            T::DocPrivate => "private",
            T::DocProtected => "protected",
            T::DocPublic => "public",
            T::DocStringBegin => "STRING_BEGIN",
            T::DocString => "STRING",
            T::DocAliasBar => "ALIAS_BAR",
            _ => KEYWORDS.iter().chain(OPERATORS).find(|(_, kind)| *kind == self).map_or("?", |(text, _)| *text),
        }
    }
}

pub struct Tokens {
    kinds: Vec<T>,
    starts: Vec<u32>,
}

impl Tokens {
    pub fn len(&self) -> usize {
        self.kinds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty()
    }

    pub fn kinds(&self) -> &[T] {
        &self.kinds
    }

    pub fn kind(&self, i: usize) -> T {
        self.kinds[i]
    }

    pub fn start(&self, i: usize) -> usize {
        self.starts[i] as usize
    }

    pub fn end(&self, i: usize) -> usize {
        self.starts[i + 1] as usize
    }

    pub fn text<'a>(&self, src: &'a str, i: usize) -> &'a str {
        &src[self.start(i)..self.end(i)]
    }

    /// The token that holds the byte `at`, or the last one when `at` is the end of the text.
    pub fn at(&self, at: usize) -> Option<usize> {
        if self.kinds.is_empty() {
            return None;
        }
        let i = self.starts.partition_point(|start| *start as usize <= at);
        Some(i.saturating_sub(1).min(self.kinds.len() - 1))
    }

    pub(super) fn with_capacity(tokens: usize) -> Self {
        Tokens { kinds: Vec::with_capacity(tokens), starts: Vec::with_capacity(tokens + 1) }
    }

    pub(super) fn push(&mut self, kind: T, start: usize) {
        self.kinds.push(kind);
        self.starts.push(start as u32);
    }

    /// Ends the last token at `end`: nothing is pushed after it.
    pub(super) fn close(&mut self, end: usize) {
        self.starts.push(end as u32);
    }
}

pub fn is_id_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

pub fn is_id_part(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Whether `name` can be written as a name of the language.
pub fn is_identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes.next().is_some_and(is_id_start) && bytes.all(is_id_part)
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c)
}

fn line_end(bytes: &[u8], from: usize) -> usize {
    bytes[from..].iter().position(|b| matches!(b, b'\r' | b'\n')).map_or(bytes.len(), |n| from + n)
}

/// `[`, any number of `=`, `[`: the level of the long bracket that opens at `at`.
pub fn long_bracket(bytes: &[u8], at: usize) -> Option<usize> {
    if bytes.get(at) != Some(&b'[') {
        return None;
    }
    let mut i = at + 1;
    while bytes.get(i) == Some(&b'=') {
        i += 1;
    }
    (bytes.get(i) == Some(&b'[')).then_some(i - at - 1)
}

fn long_end(bytes: &[u8], from: usize, level: usize) -> usize {
    let mut i = from;
    while i < bytes.len() {
        if bytes[i] != b']' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < bytes.len() && bytes[j] == b'=' {
            j += 1;
        }
        if j - i - 1 == level && bytes.get(j) == Some(&b']') {
            return j + 1;
        }
        i = j;
    }
    bytes.len()
}

/// End of the quoted literal that opens at `at`, and whether its closing quote is there.
/// A line break ends an unclosed literal; `\z` and a backslash before a line break carry on.
pub fn quoted_end(bytes: &[u8], at: usize) -> (usize, bool) {
    let quote = bytes[at];
    let mut i = at + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => match bytes.get(i + 1) {
                Some(b'\r') if bytes.get(i + 2) == Some(&b'\n') => i += 3,
                Some(b'z') => {
                    i += 2;
                    while i < bytes.len() && is_space(bytes[i]) {
                        i += 1;
                    }
                }
                Some(_) => i += 2,
                None => i += 1,
            },
            b'\r' | b'\n' => return (i, false),
            b if b == quote => return (i + 1, true),
            _ => i += 1,
        }
    }
    (bytes.len(), false)
}

fn number_end(bytes: &[u8], mut i: usize) -> usize {
    let hex = bytes[i] == b'0' && matches!(bytes.get(i + 1), Some(b'x' | b'X'));
    if hex {
        i += 2;
    }
    while i < bytes.len() {
        let b = bytes[i];
        let exponent = if hex { matches!(b, b'p' | b'P') } else { matches!(b, b'e' | b'E') };
        if exponent && matches!(bytes.get(i + 1), Some(b'+' | b'-')) {
            i += 2;
        } else if is_id_part(b) || b == b'.' {
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

fn comment(src: &str, bytes: &[u8], at: usize) -> (T, usize) {
    let rest = &src[at..];
    if rest.starts_with("--region") || rest.starts_with("--{{{") {
        return (T::Region, line_end(bytes, at));
    }
    if rest.starts_with("--endregion") || rest.starts_with("--}}}") {
        return (T::EndRegion, line_end(bytes, at));
    }
    if let Some(level) = long_bracket(bytes, at + 2) {
        return (T::BlockComment, long_end(bytes, at + 2 + level + 2, level));
    }
    if bytes.get(at + 2) != Some(&b'-') {
        return (T::ShortComment, line_end(bytes, at));
    }
    // Consecutive `---` lines are one documentation comment.
    let mut end = line_end(bytes, at);
    loop {
        let mut next = end;
        if bytes.get(next) == Some(&b'\r') {
            next += 1;
        }
        if bytes.get(next) != Some(&b'\n') {
            break;
        }
        next += 1;
        while matches!(bytes.get(next), Some(b' ' | b'\t' | 0x0c)) {
            next += 1;
        }
        if !bytes[next..].starts_with(b"---") {
            break;
        }
        end = line_end(bytes, next);
    }
    (T::DocComment, end)
}

/// Every token of `src`, white space and comments included: the tokens cover the text end to end.
pub fn lex(src: &str) -> Tokens {
    let bytes = src.as_bytes();
    let mut out = Tokens::with_capacity(src.len() / 3);
    let mut i = 0;
    if bytes.starts_with(b"#!") {
        out.push(T::Shebang, 0);
        i = line_end(bytes, 2);
        if i > 2 {
            out.push(T::ShebangContent, 2);
        }
    }
    while i < bytes.len() {
        let start = i;
        let c = bytes[i];
        let kind = match c {
            _ if is_space(c) => {
                while i < bytes.len() && is_space(bytes[i]) {
                    i += 1;
                }
                T::Whitespace
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                let (kind, end) = comment(src, bytes, i);
                i = end;
                kind
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = src[i + 2..].find("*/").map_or(bytes.len(), |n| i + 2 + n + 2);
                T::BlockComment
            }
            b'[' if long_bracket(bytes, i).is_some() => {
                let level = long_bracket(bytes, i).unwrap_or(0);
                i = long_end(bytes, i + level + 2, level);
                T::String
            }
            b'"' | b'\'' | b'`' => {
                i = quoted_end(bytes, i).0;
                if c == b'`' { T::HashString } else { T::String }
            }
            b'0'..=b'9' => {
                i = number_end(bytes, i);
                T::Number
            }
            b'.' if bytes.get(i + 1).is_some_and(u8::is_ascii_digit) => {
                i = number_end(bytes, i);
                T::Number
            }
            _ if is_id_start(c) => {
                while i < bytes.len() && is_id_part(bytes[i]) {
                    i += 1;
                }
                T::keyword(&src[start..i]).unwrap_or(T::Id)
            }
            _ => match operator(bytes, i) {
                Some((kind, len)) => {
                    i += len;
                    kind
                }
                None => {
                    i += src[i..].chars().next().map_or(1, char::len_utf8);
                    T::BadCharacter
                }
            },
        };
        out.push(kind, start);
    }
    out.close(bytes.len());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<(T, &str)> {
        let tokens = lex(src);
        (0..tokens.len())
            .filter(|i| tokens.kind(*i) != T::Whitespace)
            .map(|i| (tokens.kind(i), tokens.text(src, i)))
            .collect()
    }

    #[test]
    fn tokens_cover_the_text() {
        let src = "#!/usr/bin/lua\nlocal a <const> = `x` .. 'é' -- c\r\n--[==[ b ]==] /* c */ é ! 1..2 [[\nlong]] x";
        let tokens = lex(src);
        let joined: String = (0..tokens.len()).map(|i| tokens.text(src, i)).collect();
        assert_eq!(joined, src);
        assert_eq!(tokens.kind(0), T::Shebang);
        assert_eq!(tokens.kind(1), T::ShebangContent);
    }

    #[test]
    fn comments() {
        assert_eq!(
            kinds("-- a\n--- b\n  --- c\n-- d"),
            [(T::ShortComment, "-- a"), (T::DocComment, "--- b\n  --- c"), (T::ShortComment, "-- d"),]
        );
        assert_eq!(
            kinds("--region r\n--endregion\n--{{{\n--}}}"),
            [(T::Region, "--region r"), (T::EndRegion, "--endregion"), (T::Region, "--{{{"), (T::EndRegion, "--}}}"),]
        );
        assert_eq!(kinds("--[=[ a ]] b ]=] c"), [(T::BlockComment, "--[=[ a ]] b ]=]"), (T::Id, "c")]);
        assert_eq!(kinds("a /* b /* c */ d"), [(T::Id, "a"), (T::BlockComment, "/* b /* c */"), (T::Id, "d")]);
        assert_eq!(kinds("--[ a"), [(T::ShortComment, "--[ a")]);
        assert_eq!(kinds("--[[ open"), [(T::BlockComment, "--[[ open")]);
    }

    #[test]
    fn operators_take_the_longest_match() {
        let ops: Vec<T> =
            kinds("a <<= b >>= c // d //= e ..= f ... :: ~= ^= != ++").into_iter().map(|(k, _)| k).collect();
        assert_eq!(
            ops,
            [
                T::Id,
                T::BitLtLtAssign,
                T::Id,
                T::BitRtRtAssign,
                T::Id,
                T::DoubleDiv,
                T::Id,
                T::DoubleDiv,
                T::Assign,
                T::Id,
                T::Concat,
                T::Assign,
                T::Id,
                T::Ellipsis,
                T::DoubleColon,
                T::Ne,
                T::BitXorAssign,
                T::BadCharacter,
                T::Assign,
                T::Plus,
                T::Plus,
            ]
        );
    }

    #[test]
    fn strings() {
        assert_eq!(
            kinds("'it\\'s' \"a\\z\n  b\" `h\\`h` [==[x]]y]==]"),
            [
                (T::String, "'it\\'s'"),
                (T::String, "\"a\\z\n  b\""),
                (T::HashString, "`h\\`h`"),
                (T::String, "[==[x]]y]==]"),
            ]
        );
        assert_eq!(
            kinds("f('abc\nnext()"),
            [(T::Id, "f"), (T::LParen, "("), (T::String, "'abc"), (T::Id, "next"), (T::LParen, "("), (T::RParen, ")"),]
        );
        assert_eq!(
            kinds("t[=1]"),
            [(T::Id, "t"), (T::LBrack, "["), (T::Assign, "="), (T::Number, "1"), (T::RBrack, "]")]
        );
    }

    #[test]
    fn numbers_and_names() {
        assert_eq!(
            kinds("1..2 1e-3 0x1p4 .5 5. 0xA.8p+1 3ULL"),
            [
                (T::Number, "1"),
                (T::Concat, ".."),
                (T::Number, "2"),
                (T::Number, "1e-3"),
                (T::Number, "0x1p4"),
                (T::Number, ".5"),
                (T::Number, "5."),
                (T::Number, "0xA.8p+1"),
                (T::Number, "3ULL"),
            ]
        );
        assert_eq!(
            kinds("goto done _x1 é"),
            [(T::Goto, "goto"), (T::Id, "done"), (T::Id, "_x1"), (T::BadCharacter, "é"),]
        );
    }

    #[test]
    fn every_operator_and_keyword_lexes_to_its_kind() {
        for (text, kind) in OPERATORS.iter().chain(KEYWORDS) {
            assert_eq!(kinds(text), [(*kind, *text)]);
        }
    }

    #[test]
    fn names_follow_intellij() {
        assert_eq!(T::Assign.name(), "=");
        assert_eq!(T::DoubleDiv.name(), "//");
        assert_eq!(T::Local.name(), "local");
        assert_eq!(T::Shebang.name(), "#!");
    }
}
