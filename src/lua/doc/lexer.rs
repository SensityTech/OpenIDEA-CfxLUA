use crate::lua::token::{T, Tokens};

/// The states of the JetBrains plugin's documentation lexer (`doc.flex`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Initial,
    AfterDashes,
    Tag,
    TagWithId,
    TagName,
    CommentString,
    Param,
    TypeRef,
    Class,
    ClassAttr,
    ClassExtend,
    Field,
    FieldId,
    Generic,
    Alias,
    Suppress,
    Cast,
    OptionalMark,
    Quoted,
}

enum Step {
    Token(T, usize),
    /// The state changed and the same text is read again.
    Again,
}

struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    at: usize,
    end: usize,
    state: State,
    resumed: Vec<State>,
    level: u32,
    /// A type, or a part of one, is expected: false right after a complete type.
    wanted: bool,
    /// A `fun` keyword waits for its parameter list.
    fun_pending: bool,
    /// After `@return`, a name may follow each returned type.
    return_tag: bool,
    name_read: bool,
    /// After `@cast name`, the types may be prefixed by `+` or `-`.
    cast_tag: bool,
    /// The key type of an index signature (`@field [K] V`) is being read.
    field_index: bool,
}

fn is_id_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '$'
}

fn is_id_part(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$' || c == '.'
}

impl Lexer<'_> {
    fn byte(&self, ahead: usize) -> Option<u8> {
        (self.at + ahead < self.end).then(|| self.bytes[self.at + ahead])
    }

    fn rest(&self) -> &str {
        &self.src[self.at..self.end]
    }

    fn char_len(&self) -> usize {
        self.rest().chars().next().map_or(1, char::len_utf8)
    }

    fn eol(&self) -> usize {
        match (self.byte(0), self.byte(1)) {
            (Some(b'\r'), Some(b'\n')) => 2,
            (Some(b'\r' | b'\n'), _) => 1,
            _ => 0,
        }
    }

    fn spaces(&self) -> usize {
        self.rest().bytes().take_while(|b| matches!(b, b' ' | b'\t' | 0x0c)).count()
    }

    /// The length of the name at `ahead`: letters, then letters, digits and dots.
    fn id(&self, ahead: usize) -> usize {
        let text = &self.src[self.at + ahead..self.end];
        let mut chars = text.char_indices();
        match chars.next() {
            Some((_, c)) if is_id_start(c) => {}
            _ => return 0,
        }
        chars.find(|(_, c)| !is_id_part(*c)).map_or(text.len(), |(at, _)| at)
    }

    fn number(&self) -> usize {
        let bytes = self.rest().as_bytes();
        let sign = usize::from(bytes.first() == Some(&b'-'));
        let digits = |from: usize, hex: bool| {
            bytes[from..].iter().take_while(|b| if hex { b.is_ascii_hexdigit() } else { b.is_ascii_digit() }).count()
        };
        if bytes.get(sign) == Some(&b'0') && matches!(bytes.get(sign + 1), Some(b'x' | b'X')) {
            let hex = digits(sign + 2, true);
            if hex > 0 {
                return sign + 2 + hex;
            }
        }
        let whole = digits(sign, false);
        if whole == 0 {
            return 0;
        }
        let mut len = sign + whole;
        if bytes.get(len) == Some(&b'.') {
            let fraction = digits(len + 1, false);
            if fraction > 0 {
                len += 1 + fraction;
            }
        }
        len
    }

    /// A quoted literal: it may run over line breaks, and stops before a backslash that escapes a space.
    fn quoted(&self) -> usize {
        let text = self.rest();
        let quote = text.as_bytes()[0] as char;
        let mut chars = text.char_indices().skip(1).peekable();
        while let Some((at, c)) = chars.next() {
            if c == quote {
                return at + 1;
            }
            if c == '\\' {
                match chars.peek() {
                    Some((_, escaped)) if !escaped.is_whitespace() || matches!(escaped, '\r' | '\n') => {
                        chars.next();
                    }
                    _ => return at,
                }
            }
        }
        text.len()
    }

    fn in_type(&self) -> bool {
        self.wanted || self.level > 0
    }

    fn begin_type(&mut self) {
        self.state = State::TypeRef;
        self.level = 0;
        self.wanted = true;
        self.fun_pending = false;
        self.name_read = false;
    }

    fn reset_tag(&mut self) {
        self.return_tag = false;
        self.cast_tag = false;
        self.field_index = false;
        self.name_read = false;
        self.fun_pending = false;
    }

    /// After a complete type, the rest of the line is the description.
    fn describe(&mut self) -> Step {
        self.state = State::CommentString;
        Step::Again
    }

    fn back_to(&mut self, state: State) -> Step {
        self.state = state;
        Step::Again
    }

    /// White space, which most states share: a line break starts the next line afresh.
    fn blank(&mut self) -> Option<Step> {
        let eol = self.eol();
        if eol > 0 {
            self.state = State::Initial;
            return Some(Step::Token(T::Whitespace, eol));
        }
        let spaces = self.spaces();
        (spaces > 0).then_some(Step::Token(T::Whitespace, spaces))
    }

    fn bad(&self) -> Step {
        Step::Token(T::BadCharacter, self.char_len())
    }

    fn initial(&mut self) -> Step {
        let eol = self.eol();
        if eol > 0 {
            return Step::Token(T::Whitespace, eol);
        }
        let spaces = self.spaces();
        if spaces > 0 {
            return Step::Token(T::Whitespace, spaces);
        }
        let dashes = self.rest().bytes().take_while(|b| *b == b'-').count();
        if dashes >= 2 {
            self.reset_tag();
            self.state = State::AfterDashes;
            return Step::Token(T::DocDashes, dashes);
        }
        if self.byte(0) == Some(b'@') {
            self.state = State::TagName;
            return Step::Token(T::DocAt, 1);
        }
        self.back_to(State::CommentString)
    }

    fn after_dashes(&mut self) -> Step {
        if self.byte(0) != Some(b'|') {
            return self.back_to(State::Initial);
        }
        let len = 1 + usize::from(matches!(self.byte(1), Some(b'>' | b'+')));
        self.begin_type();
        Step::Token(T::DocAliasBar, len)
    }

    fn tag_name(&mut self) -> Step {
        let len = self.id(0);
        if len == 0 {
            return self.bad();
        }
        let (kind, state) = match &self.rest()[..len] {
            "field" => (T::DocTagField, State::Field),
            "param" => (T::DocTagParam, State::Param),
            "vararg" => (T::DocTagVararg, State::Param),
            "class" => (T::DocTagClass, State::Class),
            "enum" => (T::DocTagEnum, State::Class),
            "module" => {
                // `@module 'name'` types a local as `require('name')`: not a module declaration.
                let spaces = self.rest()[len..].bytes().take_while(|b| matches!(b, b' ' | b'\t' | 0x0c)).count();
                if spaces > 0 && matches!(self.byte(len + spaces), Some(b'"' | b'\'')) {
                    (T::DocTagName, State::CommentString)
                } else {
                    (T::DocTagModule, State::Class)
                }
            }
            "return" | "type" | "overload" => {
                let kind = match len {
                    6 => T::DocTagReturn,
                    4 => T::DocTagType,
                    _ => T::DocTagOverload,
                };
                self.begin_type();
                self.return_tag = kind == T::DocTagReturn;
                return Step::Token(kind, len);
            }
            "cast" => (T::DocTagCast, State::Cast),
            "private" => (T::DocTagPrivate, State::TagName),
            "protected" => (T::DocTagProtected, State::TagName),
            "public" => (T::DocTagPublic, State::TagName),
            "language" => (T::DocTagLanguage, State::TagWithId),
            "generic" => (T::DocTagGeneric, State::Generic),
            "see" => (T::DocTagSee, State::Tag),
            "alias" => (T::DocTagAlias, State::Alias),
            "suppress" => (T::DocTagSuppress, State::Suppress),
            _ => (T::DocTagName, State::CommentString),
        };
        self.state = state;
        Step::Token(kind, len)
    }

    /// A state that reads names and a few signs, and leaves the line to `otherwise` on anything else.
    fn names(&mut self, signs: &[(u8, T)], otherwise: State) -> Step {
        let len = self.id(0);
        if len > 0 {
            return Step::Token(T::Id, len);
        }
        match signs.iter().find(|(sign, _)| self.byte(0) == Some(*sign)) {
            Some((_, kind)) => Step::Token(*kind, 1),
            None => self.back_to(otherwise),
        }
    }

    /// `@alias name` and `@cast name`: the name, then a type.
    fn named(&mut self, cast: bool) -> Step {
        let len = self.id(0);
        if len == 0 {
            return self.back_to(State::Initial);
        }
        self.begin_type();
        self.cast_tag = cast;
        Step::Token(T::Id, len)
    }

    fn class(&mut self) -> Step {
        if self.byte(0) == Some(b'(') {
            self.state = State::ClassAttr;
            return Step::Token(T::LParen, 1);
        }
        let len = self.id(0);
        if len == 0 {
            return self.bad();
        }
        self.state = State::ClassExtend;
        Step::Token(T::Id, len)
    }

    fn class_attr(&mut self) -> Step {
        let len = self.id(0);
        if len > 0 {
            return Step::Token(T::Id, len);
        }
        match self.byte(0) {
            Some(b',') => Step::Token(T::Comma, 1),
            Some(b')') => {
                self.state = State::Class;
                Step::Token(T::RParen, 1)
            }
            _ => self.bad(),
        }
    }

    fn class_extend(&mut self) -> Step {
        if self.byte(0) != Some(b':') {
            return self.back_to(State::CommentString);
        }
        self.begin_type();
        Step::Token(T::Colon, 1)
    }

    fn param(&mut self) -> Step {
        let len = if self.rest().starts_with("...") { 3 } else { self.id(0) };
        if len == 0 {
            return self.bad();
        }
        self.state = State::OptionalMark;
        Step::Token(T::Id, len)
    }

    fn optional_mark(&mut self) -> Step {
        self.begin_type();
        if self.byte(0) == Some(b'?') { Step::Token(T::Question, 1) } else { Step::Again }
    }

    fn field(&mut self, modifiers: bool) -> Step {
        if self.byte(0) == Some(b'[') {
            self.begin_type();
            self.field_index = true;
            return Step::Token(T::LBrack, 1);
        }
        let len = self.id(0);
        if len == 0 {
            return self.bad();
        }
        let modifier = match &self.rest()[..len] {
            "private" => Some(T::DocPrivate),
            "protected" => Some(T::DocProtected),
            "public" => Some(T::DocPublic),
            _ => None,
        };
        match modifier.filter(|_| modifiers) {
            Some(kind) => {
                self.state = State::FieldId;
                Step::Token(kind, len)
            }
            None => {
                self.state = State::OptionalMark;
                Step::Token(T::Id, len)
            }
        }
    }

    /// Closes a bracket of a type, or starts the description when none is open.
    fn close(&mut self, kind: T) -> Step {
        if self.level == 0 {
            return self.describe();
        }
        self.level -= 1;
        self.wanted = false;
        Step::Token(kind, 1)
    }

    /// Opens a bracket of a type, or starts the description when no type is expected.
    fn open(&mut self, kind: T, expected: bool) -> Step {
        if !expected {
            return self.describe();
        }
        self.level += 1;
        Step::Token(kind, 1)
    }

    /// A word of a type; after a complete type, the name of a returned value or the description.
    fn word(&mut self, kind: T, len: usize, completes: bool) -> Step {
        if self.in_type() {
            if completes {
                self.wanted = false;
            }
            return Step::Token(kind, len);
        }
        if self.return_tag && !self.name_read {
            self.name_read = true;
            return Step::Token(T::Id, len);
        }
        self.describe()
    }

    fn type_ref(&mut self) -> Step {
        let Some(b) = self.byte(0) else { return Step::Again };
        match b {
            b'@' | b'#' => {
                self.state = State::CommentString;
                Step::Token(T::DocStringBegin, 1)
            }
            b',' => {
                self.wanted = true;
                self.name_read = false;
                Step::Token(T::Comma, 1)
            }
            b'|' | b':' => {
                self.wanted = true;
                Step::Token(if b == b'|' { T::BitOr } else { T::Colon }, 1)
            }
            b'?' => Step::Token(T::Question, 1),
            b'<' => {
                self.level += 1;
                Step::Token(T::Lt, 1)
            }
            b'>' => self.close(T::Gt),
            b'(' => {
                let expected = self.in_type() || self.fun_pending;
                self.fun_pending = false;
                self.open(T::LParen, expected)
            }
            b')' => self.close(T::RParen),
            b'{' => self.open(T::LCurly, self.in_type()),
            b'}' => self.close(T::RCurly),
            b'[' if self.byte(1) == Some(b']') => {
                self.wanted = false;
                Step::Token(T::DocArr, 2)
            }
            b'[' => {
                let expected = self.in_type();
                self.wanted = self.wanted || expected;
                self.open(T::LBrack, expected)
            }
            b']' if self.field_index && self.level == 0 => {
                self.field_index = false;
                self.wanted = true;
                Step::Token(T::RBrack, 1)
            }
            b']' => self.close(T::RBrack),
            b'"' | b'\'' => {
                if !self.in_type() {
                    return self.describe();
                }
                self.resumed.push(self.state);
                self.back_to(State::Quoted)
            }
            b'+' | b'-' | b'0'..=b'9' => {
                let number = self.number();
                if number > 0 {
                    if !self.in_type() {
                        return self.describe();
                    }
                    self.wanted = false;
                    return Step::Token(T::Number, number);
                }
                if self.cast_tag && self.wanted {
                    return Step::Token(if b == b'+' { T::Plus } else { T::Minus }, 1);
                }
                self.describe()
            }
            b'`' => {
                // A generic capture: `T` between backticks.
                let name = self.id(1);
                if name == 0 || self.byte(1 + name) != Some(b'`') {
                    return if self.in_type() { self.bad() } else { self.describe() };
                }
                if !self.in_type() {
                    return self.describe();
                }
                self.wanted = false;
                Step::Token(T::Id, name + 2)
            }
            _ if self.rest().starts_with("...") => self.word(T::Id, 3, true),
            _ => {
                let len = self.id(0);
                match &self.rest()[..len] {
                    "" => {
                        if self.in_type() {
                            self.bad()
                        } else {
                            self.describe()
                        }
                    }
                    "fun" => {
                        let typed = self.in_type();
                        let step = self.word(T::DocFun, len, false);
                        self.fun_pending = self.fun_pending || typed;
                        step
                    }
                    "vararg" => {
                        let typed = self.in_type();
                        let step = self.word(T::DocVararg, len, false);
                        self.wanted = self.wanted || typed;
                        step
                    }
                    _ => self.word(T::Id, len, true),
                }
            }
        }
    }

    /// After `@see`: names and `#`, then the description.
    fn tag(&mut self) -> Step {
        if self.byte(0) == Some(b'@') {
            self.state = State::CommentString;
            return Step::Token(T::DocStringBegin, 1);
        }
        self.names(&[(b'#', T::GetN)], State::CommentString)
    }

    /// After `@language`: a name or a quoted one.
    fn tag_with_id(&mut self) -> Step {
        if matches!(self.byte(0), Some(b'"' | b'\'')) {
            self.resumed.push(self.state);
            return self.back_to(State::Quoted);
        }
        let len = self.id(0);
        if len == 0 {
            return self.bad();
        }
        self.state = State::CommentString;
        Step::Token(T::Id, len)
    }

    /// The description: the longest of the white space and of the text up to a tab or the end of the line.
    fn comment_string(&mut self) -> Step {
        let eol = self.eol();
        if eol > 0 {
            self.state = State::Initial;
            return Step::Token(T::Whitespace, eol);
        }
        let spaces = self.spaces();
        let text = self.rest().bytes().take_while(|b| !matches!(b, b'\r' | b'\n' | b'\t' | 0x0c)).count();
        if spaces >= text {
            return Step::Token(T::Whitespace, spaces);
        }
        self.state = State::Initial;
        Step::Token(T::DocString, text)
    }

    fn quoted_string(&mut self) -> Step {
        let len = self.quoted();
        self.state = self.resumed.pop().unwrap_or(State::Initial);
        self.wanted = false;
        Step::Token(T::String, len)
    }

    fn step(&mut self) -> Step {
        let shared = !matches!(
            self.state,
            State::Initial | State::AfterDashes | State::OptionalMark | State::Quoted | State::CommentString
        );
        if shared && let Some(step) = self.blank() {
            return step;
        }
        match self.state {
            State::Initial => self.initial(),
            State::AfterDashes => self.after_dashes(),
            State::TagName => self.tag_name(),
            State::Suppress => self.names(&[(b',', T::Comma)], State::Initial),
            State::Generic => self.names(&[(b':', T::Colon), (b',', T::Comma)], State::Initial),
            State::Alias => self.named(false),
            State::Cast => self.named(true),
            State::Class => self.class(),
            State::ClassAttr => self.class_attr(),
            State::ClassExtend => self.class_extend(),
            State::Param => self.param(),
            State::OptionalMark => self.optional_mark(),
            State::Field => self.field(true),
            State::FieldId => self.field(false),
            State::TypeRef => self.type_ref(),
            State::Tag => self.tag(),
            State::TagWithId => self.tag_with_id(),
            State::CommentString => self.comment_string(),
            State::Quoted => self.quoted_string(),
        }
    }
}

/// Adds to `out` the tokens of the documentation comment `src[from..to]`: they cover it end to end.
pub(super) fn lex(src: &str, from: usize, to: usize, out: &mut Tokens) {
    let mut lexer = Lexer {
        src,
        bytes: src.as_bytes(),
        at: from,
        end: to,
        state: State::Initial,
        resumed: Vec::new(),
        level: 0,
        wanted: false,
        fun_pending: false,
        return_tag: false,
        name_read: false,
        cast_tag: false,
        field_index: false,
    };
    while lexer.at < lexer.end {
        if let Step::Token(kind, len) = lexer.step() {
            out.push(kind, lexer.at);
            lexer.at += len.max(1);
        }
    }
}
