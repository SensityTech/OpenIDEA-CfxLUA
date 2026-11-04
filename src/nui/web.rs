//! Facts of a web file (JavaScript, TypeScript, JSX, Vue, Svelte, HTML
//! scripts) about the NUI bridge, read from its tokens: callback URLs, calls
//! with constant names, wrappers (`fetchNui`, `useNuiEvent`), message
//! listeners, JS client script registrations, imports. They depend on the
//! file only; wrappers and imports are resolved project-wide by the bridge.

use super::bridge::{At, WrapKind};
use super::js::{Kind, Tok, tokenize};
use crate::lua::Source;

/// `https://${GetParentResourceName()}/close`; `host`: the resource named, `None` for the page's own.
#[derive(Clone, Debug, PartialEq)]
pub struct Url {
    pub name: String,
    pub at: At,
    pub host: Option<String>,
}

/// `fetchNui('close')`, `api.post('close')`; `receiver`: `?` for an expression, `None` for a plain call.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub callee: String,
    pub receiver: Option<String>,
    pub arg: usize,
    pub value: String,
    pub at: At,
}

/// A function forwarding its parameter `param` as a callback name (it builds
/// the URL from it) or a message name (its listener compares the message to
/// it, through `field`).
#[derive(Clone, Debug, PartialEq)]
pub struct Wrapper {
    pub kind: WrapKind,
    pub name: String,
    pub container: Option<String>,
    pub param: usize,
    pub field: Option<String>,
}

/// Function `name` passes its parameter `param` as argument `arg` of a call.
#[derive(Clone, Debug, PartialEq)]
pub struct Forward {
    pub name: String,
    pub container: Option<String>,
    pub param: usize,
    pub callee: String,
    pub receiver: Option<String>,
    pub arg: usize,
}

/// `case 'open':`, `event.data.action === 'open'`; `field` is the compared field.
#[derive(Clone, Debug, PartialEq)]
pub struct Listen {
    pub field: String,
    pub name: String,
    pub at: At,
}

/// `RegisterNuiCallbackType('close')` in a JavaScript client script.
#[derive(Clone, Debug, PartialEq)]
pub struct Registration {
    pub name: String,
    pub at: At,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Facts {
    pub urls: Vec<Url>,
    pub calls: Vec<Call>,
    pub wrappers: Vec<Wrapper>,
    pub forwards: Vec<Forward>,
    pub listens: Vec<Listen>,
    pub registrations: Vec<Registration>,
    /// The file uses `import` / `export`.
    pub module: bool,
    /// Local binding → imported name (`default`, `*` for a namespace), later ones overriding.
    pub imports: Vec<(String, String)>,
    /// Callbacks registered with names computed at runtime.
    pub dynamic: usize,
}

fn set_import(imports: &mut Vec<(String, String)>, local: String, imported: String) {
    match imports.iter_mut().find(|(l, _)| *l == local) {
        Some(e) => e.1 = imported,
        None => imports.push((local, imported)),
    }
}

const MARKUP: &[&str] = &["vue", "svelte", "html", "htm"];

/// Runtime functions registering a NUI callback from a JavaScript client script.
const REGISTER_FUNCTIONS: &[&str] =
    &["RegisterNuiCallbackType", "RegisterNUICallback", "RegisterNuiCallback", "RegisterRawNuiCallback"];
pub const NUI_EVENT_PREFIX: &str = "__cfx_nui:";
const NUI_EVENT_FUNCTIONS: &[&str] = &["on", "AddEventHandler", "addEventListener"];
const LISTENER_REGISTRATIONS: &[&str] = &["addEventListener", "on", "addListener"];
const RESOURCE_FUNCTION: &str = "GetParentResourceName";
const MAX_ARGS: usize = 4;
const MAX_DEPTH: usize = 4;
const MAX_TYPE_TOKENS: usize = 60;
const MAX_LOOKBACK: usize = 4000;
const RESOURCE_MARK: char = '\u{1}';
const PARAM_MARK: char = '\u{2}';
const DYNAMIC_MARK: char = '\u{3}';
const OPENERS: &[&str] = &["(", "[", "{"];
const COMPARISONS: &[&str] = &["===", "==", "!==", "!="];
const GLOBAL_OBJECTS: &[&str] = &["window", "globalThis", "self", "module", "exports", "this"];
const METHOD_MODIFIERS: &[&str] =
    &["static", "async", "get", "set", "public", "private", "protected", "override", "readonly", "abstract"];
const PARAM_MODIFIERS: &[&str] = &["public", "private", "protected", "readonly", "override"];
const TYPE_PUNCTUATION: &[&str] =
    &[",", ".", "[", "]", "{", "}", "(", ")", ":", ";", "|", "&", "?", "=>", "=", "...", "<", ">", "?."];
const DECLARATIONS: &[&str] = &["const", "let", "var"];

/// Identifiers that are not names.
const KEYWORDS: &[&str] = &[
    "if",
    "for",
    "while",
    "switch",
    "catch",
    "function",
    "return",
    "typeof",
    "new",
    "do",
    "else",
    "with",
    "await",
    "yield",
    "import",
    "export",
    "class",
    "const",
    "let",
    "var",
    "case",
    "delete",
    "void",
    "in",
    "of",
    "instanceof",
    "throw",
    "super",
    "this",
    "try",
    "finally",
    "break",
    "continue",
    "default",
    "extends",
    "true",
    "false",
    "null",
    "undefined",
    "debugger",
];

/// The script ranges of a file with extension `ext`: the whole text, or its `<script>` blocks.
pub fn script_ranges(text: &str, ext: &str) -> Vec<(usize, usize)> {
    if !MARKUP.contains(&ext.to_ascii_lowercase().as_str()) {
        return vec![(0, text.len())];
    }
    let lower = text.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(open_end) = script_open(&lower, from) {
        match script_close(&lower, open_end) {
            Some((close_start, close_end)) => {
                out.push((open_end, close_start));
                from = close_end;
            }
            None => {
                out.push((open_end, text.len()));
                break;
            }
        }
    }
    out
}

/// End of the next `<script\b[^>]*>` from `from` (lowercase text).
fn script_open(lower: &str, mut from: usize) -> Option<usize> {
    loop {
        let at = from + lower.get(from..)?.find("<script")?;
        let after = at + "<script".len();
        let boundary = lower.as_bytes().get(after).is_none_or(|b| !(b.is_ascii_alphanumeric() || *b == b'_'));
        if boundary {
            return lower[after..].find('>').map(|g| after + g + 1);
        }
        from = after;
    }
}

/// Start and end of the next `</script\s*>` from `from` (lowercase text).
fn script_close(lower: &str, mut from: usize) -> Option<(usize, usize)> {
    loop {
        let at = from + lower.get(from..)?.find("</script")?;
        let mut i = at + "</script".len();
        while lower.as_bytes().get(i).is_some_and(|b| b.is_ascii_whitespace()) {
            i += 1;
        }
        if lower.as_bytes().get(i) == Some(&b'>') {
            return Some((at, i + 1));
        }
        from = at + 1;
    }
}

/// Whether a file looks generated (a minified bundle): it would duplicate the sources.
pub fn is_generated(file_name: &str, text: &str) -> bool {
    if file_name.to_ascii_lowercase().ends_with(".min.js") || text.len() > 3_000_000 {
        return true;
    }
    let lines = text.split('\n').count();
    let long = text.split('\n').any(|line| line.len() > 1_000);
    long && text.len() / lines > 200
}

/// The facts of a web file with extension `ext`.
pub fn scan(src: &Source, ext: &str) -> Facts {
    let mut facts = Facts::default();
    for (from, to) in script_ranges(&src.text, ext) {
        let toks = tokenize(&src.text, from, to);
        let part = Analysis::new(&src.text, &toks).facts(src);
        facts.urls.extend(part.urls);
        facts.calls.extend(part.calls);
        facts.wrappers.extend(part.wrappers);
        facts.forwards.extend(part.forwards);
        facts.listens.extend(part.listens);
        facts.registrations.extend(part.registrations);
        facts.module |= part.module;
        for (l, i) in part.imports {
            set_import(&mut facts.imports, l, i);
        }
        facts.dynamic += part.dynamic;
    }
    facts
}

/// A function of the analysed script.
#[derive(Clone, Debug)]
struct JsFn {
    name: Option<String>,
    container: Option<String>,
    /// `None` for a destructured parameter.
    params: Vec<Option<String>>,
    /// First token (its `async`, `function` or parameters).
    head: usize,
    body_start: usize,
    /// Inclusive.
    body_end: usize,
}

/// A piece of a string computed from literals and values.
#[derive(Clone, Debug)]
enum Piece {
    /// Its text and the offset of its first character, when written as is.
    Lit(String, Option<usize>),
    /// `GetParentResourceName()`.
    Resource,
    Param(usize, usize),
    Dynamic,
}

struct Analysis<'a> {
    text: &'a str,
    t: &'a [Tok],
    pair: Vec<Option<usize>>,
    /// Innermost `{` enclosing each token.
    brace: Vec<Option<usize>>,
    fns: Vec<JsFn>,
}

impl<'a> Analysis<'a> {
    fn new(text: &'a str, t: &'a [Tok]) -> Self {
        let (pair, brace) = brackets(text, t);
        let mut a = Self { text, t, pair, brace, fns: Vec::new() };
        a.find_functions();
        a
    }

    fn tok(&self, i: usize) -> Option<&Tok> {
        self.t.get(i)
    }

    fn txt(&self, i: usize) -> &str {
        self.t.get(i).map_or("", |t| t.text(self.text))
    }

    fn punct(&self, i: usize, p: &str) -> bool {
        self.t.get(i).is_some_and(|t| t.is_punct(self.text, p))
    }

    /// `punct` at `i - d` (false before the start).
    fn punct_back(&self, i: usize, d: usize, p: &str) -> bool {
        i >= d && self.punct(i - d, p)
    }

    fn ident(&self, i: usize, name: &str) -> bool {
        self.t.get(i).is_some_and(|t| t.is_ident(self.text, name))
    }

    fn ident_back(&self, i: usize, d: usize, name: &str) -> bool {
        i >= d && self.ident(i - d, name)
    }

    fn is_name(&self, i: usize) -> bool {
        self.t.get(i).is_some_and(|t| t.kind == Kind::Ident && !KEYWORDS.contains(&t.text(self.text)))
    }

    fn is_member(&self, i: usize) -> bool {
        self.punct_back(i, 1, ".") || self.punct_back(i, 1, "?.")
    }

    fn declared(&self, i: usize) -> bool {
        DECLARATIONS.iter().any(|d| self.ident_back(i, 1, d))
    }

    // ---- functions ----------------------------------------------------------------------------

    fn find_functions(&mut self) {
        for i in 0..self.t.len() {
            if self.ident(i, "function") {
                self.function_keyword(i);
            } else if self.punct(i, "=>") {
                self.arrow(i);
            } else if self.is_name(i) {
                self.method(i);
            }
        }
    }

    fn function_keyword(&mut self, i: usize) {
        let mut j = i + 1;
        if self.punct(j, "*") {
            j += 1;
        }
        let mut name = None;
        if self.is_name(j) && (self.punct(j + 1, "(") || self.punct(j + 1, "<")) {
            name = Some(self.txt(j).to_owned());
            j += 1;
        }
        if self.punct(j, "<") {
            let Some(g) = self.skip_generics(j) else { return };
            j = g;
        }
        if !self.punct(j, "(") {
            return;
        }
        let Some(close) = self.pair[j] else { return };
        let params = self.params(j + 1, close);
        let mut k = close + 1;
        if self.punct(k, ":") {
            let Some(b) = self.skip_type_to_brace(k + 1) else { return };
            k = b;
        }
        if !self.punct(k, "{") {
            return;
        }
        let Some(end) = self.pair[k] else { return };
        let head = if self.ident_back(i, 1, "async") { i - 1 } else { i };
        let mut container = None;
        if name.is_none() {
            (name, container) = self.name_from_context(head).unwrap_or_default();
        }
        self.fns.push(JsFn { name, container, params, head, body_start: k, body_end: end });
    }

    fn arrow(&mut self, i: usize) {
        if i == 0 {
            return;
        }
        let params;
        let mut head;
        if self.is_name(i - 1) {
            params = vec![Some(self.txt(i - 1).to_owned())];
            head = i - 1;
        } else {
            let mut close = self.punct(i - 1, ")").then_some(i - 1);
            if close.is_none() {
                // `(a: string): Promise<void> =>`
                let low = i.checked_sub(MAX_TYPE_TOKENS);
                let mut q = i - 1;
                while low.is_none_or(|l| q > l) {
                    if self.punct(q, ";") || self.punct(q, "{") || self.punct(q, "=") {
                        break;
                    }
                    if self.punct(q, ")") && self.punct(q + 1, ":") {
                        close = Some(q);
                        break;
                    }
                    let Some(prev) = q.checked_sub(1) else { break };
                    q = prev;
                }
            }
            let Some(close) = close else { return };
            let Some(open) = self.pair[close] else { return };
            params = self.params(open + 1, close);
            head = open;
            // `<T = unknown>(action: string) =>`
            if self.punct_back(head, 1, ">")
                && let Some(g) = self.generics_start(head - 1)
            {
                head = g;
            }
        }
        if self.ident_back(head, 1, "async") {
            head -= 1;
        }
        let (body_start, body_end) = match self.pair.get(i + 1).copied().flatten() {
            Some(end) if self.punct(i + 1, "{") => (i + 1, end),
            _ => {
                let e = self.expression_end(i + 1, false);
                if e <= i + 1 {
                    return;
                }
                (i + 1, e - 1)
            }
        };
        let (name, container) = self.name_from_context(head).unwrap_or_default();
        self.fns.push(JsFn { name, container, params, head, body_start, body_end });
    }

    /// `name(params) {` in a class body or an object literal.
    fn method(&mut self, i: usize) {
        if i > 0 {
            let prev = &self.t[i - 1];
            let ok = match prev.kind {
                Kind::Punct => matches!(prev.text(self.text), "{" | "}" | ";" | "," | ")" | "*"),
                Kind::Ident => METHOD_MODIFIERS.contains(&prev.text(self.text)),
                _ => false,
            };
            if !ok {
                return;
            }
        }
        let mut j = i + 1;
        if self.punct(j, "<") {
            let Some(g) = self.skip_generics(j) else { return };
            j = g;
        }
        if !self.punct(j, "(") {
            return;
        }
        let Some(close) = self.pair[j] else { return };
        let mut k = close + 1;
        if self.punct(k, ":") {
            let Some(b) = self.skip_type_to_brace(k + 1) else { return };
            k = b;
        }
        if !self.punct(k, "{") {
            return;
        }
        let Some(end) = self.pair[k] else { return };
        let Some(enclosing) = self.brace[i] else { return };
        if !self.is_object_or_class(enclosing) {
            return;
        }
        let container = self.owner_of(enclosing);
        let params = self.params(j + 1, close);
        self.fns.push(JsFn {
            name: Some(self.txt(i).to_owned()),
            container,
            params,
            head: i,
            body_start: k,
            body_end: end,
        });
    }

    fn is_object_or_class(&self, open: usize) -> bool {
        if open == 0 {
            return false;
        }
        if self.class_before(open).is_some() {
            return true;
        }
        let p = open - 1;
        ["=", ":", "(", ",", "[", "?", "||", "??"].iter().any(|s| self.punct(p, s))
            || self.ident(p, "return")
            || self.ident(p, "default")
    }

    /// The class name (empty for an anonymous class) when the `{` at `open` is a class body.
    fn class_before(&self, open: usize) -> Option<String> {
        let mut q = open;
        while q > 0 && q > open.saturating_sub(MAX_TYPE_TOKENS - 1) {
            q -= 1;
            if self.punct(q, ";") || self.punct(q, "{") || self.punct(q, "}") || self.punct(q, "=>") {
                return None;
            }
            if self.ident(q, "class") {
                if self.is_name(q + 1) && !self.ident(q + 1, "extends") && !self.ident(q + 1, "implements") {
                    return Some(self.txt(q + 1).to_owned());
                }
                return Some(self.name_from_context(q).and_then(|(n, _)| n).unwrap_or_default());
            }
        }
        None
    }

    /// Name of the class or object literal opened at `open`.
    fn owner_of(&self, open: usize) -> Option<String> {
        if let Some(c) = self.class_before(open) {
            return (!c.is_empty()).then_some(c);
        }
        let p = open.checked_sub(1)?;
        if self.punct(p, "=") {
            return self.name_before_assign(p).and_then(|(n, _)| n);
        }
        if self.punct(p, ":") && p > 0 && self.is_name(p - 1) {
            return Some(self.txt(p - 1).to_owned());
        }
        None
    }

    /// The name a function expression starting at `head` gets: `const f = …`, `obj.f = …`, `{ f: … }`.
    fn name_from_context(&self, head: usize) -> Option<(Option<String>, Option<String>)> {
        let p = head.checked_sub(1)?;
        if self.punct(p, "=") {
            return self.name_before_assign(p);
        }
        if self.punct(p, ":") && p > 0 && (self.is_name(p - 1) || self.t[p - 1].kind == Kind::Str) {
            let key = &self.t[p - 1];
            let name = key.value().map_or_else(|| key.text(self.text).to_owned(), str::to_owned);
            let container = self.brace[p].and_then(|b| self.owner_of(b));
            return Some((Some(name), container));
        }
        None
    }

    /// `name =`, `obj.name =`, `const name: Type =` before the `=` at `eq`.
    fn name_before_assign(&self, eq: usize) -> Option<(Option<String>, Option<String>)> {
        let k = eq.checked_sub(1)?;
        if self.is_name(k) {
            if self.is_member(k) {
                let name = self.txt(k);
                if name == "exports" {
                    return None;
                }
                let owner = k
                    .checked_sub(2)
                    .and_then(|o| self.tok(o))
                    .filter(|t| t.kind == Kind::Ident)
                    .map(|t| t.text(self.text));
                return Some((Some(name.to_owned()), owner.filter(|o| !GLOBAL_OBJECTS.contains(o)).map(str::to_owned)));
            }
            return Some((Some(self.txt(k).to_owned()), None));
        }
        let mut q = k;
        while q > 0 && q > eq.saturating_sub(MAX_TYPE_TOKENS) {
            if self.punct(q, ";") || self.punct(q, "{") || self.punct(q, "}") {
                break;
            }
            if self.is_name(q) && self.punct(q + 1, ":") && self.declared(q) {
                return Some((Some(self.txt(q).to_owned()), None));
            }
            q -= 1;
        }
        None
    }

    /// Parameter names between `from` and `close` (exclusive), `None` for a destructured one; `this` is not one.
    fn params(&self, from: usize, close: usize) -> Vec<Option<String>> {
        let mut out = Vec::new();
        let mut i = from;
        while i < close {
            let mut j = i;
            while j < close && (self.punct(j, "@") || (PARAM_MODIFIERS.contains(&self.txt(j)) && self.is_name(j + 1))) {
                if self.punct(j, "@") {
                    j += 2;
                    if self.punct(j, "(")
                        && let Some(c) = self.pair[j]
                    {
                        j = c + 1;
                    }
                } else {
                    j += 1;
                }
            }
            if self.punct(j, "...") {
                j += 1;
            }
            let name = self.tok(j).filter(|t| j < close && t.kind == Kind::Ident).map(|t| t.text(self.text));
            if name != Some("this") {
                out.push(name.map(str::to_owned));
            }
            let mut k = j;
            while k < close && !self.punct(k, ",") {
                k = match self.pair[k] {
                    Some(b) if b > k && OPENERS.contains(&self.txt(k)) => b + 1,
                    _ => k + 1,
                };
            }
            i = k + 1;
        }
        out
    }

    /// Index after the `<…>` starting at `open`, `None` when it is not a type argument list.
    fn skip_generics(&self, open: usize) -> Option<usize> {
        let mut depth = 0usize;
        let mut i = open;
        while i < self.t.len() && i < open + MAX_TYPE_TOKENS {
            let t = &self.t[i];
            let text = t.text(self.text);
            if t.kind == Kind::Punct {
                match text {
                    "<" => depth += 1,
                    ">" => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            return Some(i + 1);
                        }
                    }
                    _ if OPENERS.contains(&text) => i = self.pair[i]?,
                    _ if !TYPE_PUNCTUATION.contains(&text) => return None,
                    _ => {}
                }
            }
            i += 1;
        }
        None
    }

    /// The `<` opening the type parameters closed by the `>` at `close`.
    fn generics_start(&self, close: usize) -> Option<usize> {
        let mut depth = 0usize;
        let low = close.checked_sub(MAX_TYPE_TOKENS);
        let mut i = close;
        loop {
            if low.is_some_and(|l| i <= l) {
                return None;
            }
            let t = &self.t[i];
            let text = t.text(self.text);
            if t.kind == Kind::Punct {
                match text {
                    ">" => depth += 1,
                    "<" => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            return Some(i);
                        }
                    }
                    _ if !TYPE_PUNCTUATION.contains(&text) => return None,
                    _ => {}
                }
            }
            i = i.checked_sub(1)?;
        }
    }

    /// The `{` ending a return type annotation starting at `from`.
    fn skip_type_to_brace(&self, from: usize) -> Option<usize> {
        let mut i = from;
        let mut angle = 0isize;
        while i < self.t.len() && i < from + MAX_TYPE_TOKENS {
            let t = &self.t[i];
            if t.kind == Kind::Punct {
                match t.text(self.text) {
                    "<" => angle += 1,
                    ">" => angle -= 1,
                    "{" if angle <= 0 && i > from => return Some(i),
                    "(" | "[" | "{" => i = self.pair[i]?,
                    ";" | "=>" | "=" => return None,
                    _ => {}
                }
            }
            i += 1;
        }
        None
    }

    /// End (exclusive) of the expression starting at `start`: a `,` or `;` at its level or a
    /// closing bracket; with `asi`, also a line starting a new statement.
    fn expression_end(&self, start: usize, asi: bool) -> usize {
        let n = self.t.len();
        let mut i = start;
        while i < n {
            let t = &self.t[i];
            if t.kind == Kind::Punct {
                match t.text(self.text) {
                    "," | ";" | ")" | "]" | "}" => return i,
                    "(" | "[" | "{" => match self.pair[i] {
                        Some(b) => {
                            i = b + 1;
                            continue;
                        }
                        None => return n,
                    },
                    _ => {}
                }
            }
            if asi && i > start && t.newline && ends_operand(self.text, &self.t[i - 1]) && starts_statement(t) {
                return i;
            }
            i += 1;
        }
        n
    }

    /// Functions whose body contains token `p`, innermost first.
    fn enclosing(&self, p: usize) -> Vec<usize> {
        let mut out: Vec<usize> =
            (0..self.fns.len()).filter(|&f| (self.fns[f].body_start..=self.fns[f].body_end).contains(&p)).collect();
        out.sort_by(|a, b| self.fns[*b].body_start.cmp(&self.fns[*a].body_start));
        out
    }

    /// The parameter `name` of a function enclosing token `p`: (function, index).
    fn param_ref(&self, name: &str, p: usize) -> Option<(usize, usize)> {
        self.enclosing(p)
            .into_iter()
            .find_map(|f| self.fns[f].params.iter().position(|q| q.as_deref() == Some(name)).map(|i| (f, i)))
    }

    /// The initializer of `const|let|var name = …` declared before `p`.
    fn initializer(&self, name: &str, p: usize) -> Option<(usize, usize)> {
        let stop = p.saturating_sub(MAX_LOOKBACK);
        for q in (stop..p.min(self.t.len())).rev() {
            if !(self.is_name(q) && self.txt(q) == name && self.declared(q)) {
                continue;
            }
            let mut e = q + 1;
            if self.punct(e, ":") {
                while e < self.t.len() && e < q + MAX_TYPE_TOKENS && !self.punct(e, "=") && !self.punct(e, ";") {
                    e += 1;
                }
            }
            if !self.punct(e, "=") {
                return None;
            }
            let end = self.expression_end(e + 1, true);
            return (end > e + 1).then_some((e + 1, end));
        }
        None
    }

    // ---- values -------------------------------------------------------------------------------

    fn lit(tk: &Tok) -> Vec<Piece> {
        match tk.value() {
            Some(v) => {
                let plain = tk.constant_is_plain();
                vec![Piece::Lit(v.to_owned(), plain.then(|| tk.content_start()))]
            }
            None => vec![Piece::Dynamic],
        }
    }

    fn template(&self, tk: &Tok, p: usize, depth: usize) -> Vec<Piece> {
        let Some(t) = tk.template() else { return vec![Piece::Dynamic] };
        let mut out = Vec::new();
        for (k, q) in t.quasis.iter().enumerate() {
            if !q.text.is_empty() {
                out.push(Piece::Lit(q.text.clone(), q.plain.then_some(q.start)));
            }
            if let Some(e) = t.exprs.get(k) {
                out.extend(self.evaluate(e, p, depth));
            }
        }
        out
    }

    /// The pieces of the expression `tokens`, read at token `p`.
    fn evaluate(&self, tokens: &[Tok], p: usize, depth: usize) -> Vec<Piece> {
        if tokens.is_empty() || depth > MAX_DEPTH {
            return vec![Piece::Dynamic];
        }
        if tokens.iter().any(|t| t.is_ident(self.text, RESOURCE_FUNCTION)) {
            return vec![Piece::Resource];
        }
        if let [tk] = tokens {
            return match tk.kind {
                Kind::Str => Self::lit(tk),
                Kind::Template => self.template(tk, p, depth),
                Kind::Ident => self.identifier(tk.text(self.text), p, depth),
                _ => vec![Piece::Dynamic],
            };
        }
        // `a + b + c` of simple operands.
        let plus = tokens.iter().any(|t| t.is_punct(self.text, "+"));
        let brackets = tokens.iter().any(|t| t.kind == Kind::Punct && OPENERS.contains(&t.text(self.text)));
        if plus && !brackets {
            let mut out = Vec::new();
            for operand in tokens.split(|t| t.is_punct(self.text, "+")) {
                if operand.is_empty() {
                    return vec![Piece::Dynamic];
                }
                out.extend(self.evaluate(operand, p, depth + 1));
            }
            return out;
        }
        vec![Piece::Dynamic]
    }

    fn identifier(&self, name: &str, p: usize, depth: usize) -> Vec<Piece> {
        if let Some((f, i)) = self.param_ref(name, p) {
            return vec![Piece::Param(f, i)];
        }
        match self.initializer(name, p) {
            Some((a, b)) => self.evaluate(&self.t[a..b], p, depth + 1),
            None => vec![Piece::Dynamic],
        }
    }

    /// Last token of the operand of a `+` concatenation starting at `i`.
    fn operand_end(&self, i: usize) -> usize {
        let mut j = i;
        if self.tok(j).is_some_and(|t| matches!(t.kind, Kind::Str | Kind::Template)) {
            return j;
        }
        if self.punct(j, "(")
            && let Some(c) = self.pair[j]
        {
            j = c;
        }
        while j + 1 < self.t.len() {
            if self.punct(j + 1, ".") || self.punct(j + 1, "?.") {
                j += 2;
            } else if (self.punct(j + 1, "(") || self.punct(j + 1, "[")) && self.pair[j + 1].is_some() {
                j = self.pair[j + 1].unwrap_or(j + 1);
            } else {
                return j;
            }
        }
        j.min(self.t.len().saturating_sub(1))
    }

    // ---- facts --------------------------------------------------------------------------------

    fn facts(&self, src: &Source) -> Facts {
        let mut f = Facts::default();
        let at = |start: usize, len: usize, exact: bool, end: usize| At::new(src, start, len, exact, end);
        for i in 0..self.t.len() {
            let tk = &self.t[i];
            if matches!(tk.kind, Kind::Str | Kind::Template) && !self.punct_back(i, 1, "+") && starts_url(tk) {
                let pieces = self.url(i);
                self.nui_url(&pieces, tk, src, &mut f);
            }
            let Some(open) = self.call_open(i) else { continue };
            let Some(close) = self.pair[open] else { continue };
            let callee = tk.text(self.text).to_owned();
            let receiver = self.receiver(i);
            let args = self.arguments(open, close);
            for (k, &(a, b)) in args.iter().enumerate().take(MAX_ARGS) {
                let single = (b == a + 1).then(|| &self.t[a]);
                if let Some(s) = single
                    && let Some(c) = s.constant()
                {
                    let plain = s.constant_is_plain();
                    let start = if plain { s.content_start() } else { s.start };
                    f.calls.push(Call {
                        callee: callee.clone(),
                        receiver: receiver.clone(),
                        arg: k,
                        value: c.to_owned(),
                        at: at(start, c.len(), plain, s.end),
                    });
                } else if single.is_some()
                    && self.is_name(a)
                    && let Some((fi, index)) = self.param_ref(self.txt(a), i)
                    && let Some(name) = &self.fns[fi].name
                {
                    let fw = Forward {
                        name: name.clone(),
                        container: self.fns[fi].container.clone(),
                        param: index,
                        callee: callee.clone(),
                        receiver: receiver.clone(),
                        arg: k,
                    };
                    if !f.forwards.contains(&fw) {
                        f.forwards.push(fw);
                    }
                }
            }
            let first = args.first().copied();
            let first_tok = first.filter(|(a, b)| *b == a + 1).map(|(a, _)| &self.t[a]);
            let first_constant = first_tok.and_then(Tok::constant);
            if REGISTER_FUNCTIONS.contains(&callee.as_str()) && receiver.is_none() && first.is_some() {
                match (first_tok, first_constant) {
                    (Some(t), Some(c)) => {
                        let plain = t.constant_is_plain();
                        let start = if plain { t.content_start() } else { t.start };
                        f.registrations.push(Registration { name: c.to_owned(), at: at(start, c.len(), plain, t.end) });
                    }
                    _ => f.dynamic += 1,
                }
            }
            if let (Some(t), Some(c)) = (first_tok, first_constant)
                && NUI_EVENT_FUNCTIONS.contains(&callee.as_str())
                && let Some(name) = c.strip_prefix(NUI_EVENT_PREFIX)
                && !name.is_empty()
            {
                let plain = t.constant_is_plain();
                let start = if plain { t.content_start() + NUI_EVENT_PREFIX.len() } else { t.start };
                f.registrations.push(Registration { name: name.to_owned(), at: at(start, name.len(), plain, t.end) });
            }
        }
        for h in self.message_handlers() {
            self.listeners(h, src, &mut f);
        }
        f.module = self.is_module();
        f.imports = self.imports();
        f
    }

    /// The receiver of a call whose callee is token `i`.
    fn receiver(&self, i: usize) -> Option<String> {
        if !self.is_member(i) {
            return None;
        }
        let r = i.checked_sub(2).and_then(|r| self.tok(r)).filter(|t| t.kind == Kind::Ident);
        Some(r.map_or_else(|| "?".to_owned(), |t| t.text(self.text).to_owned()))
    }

    /// The `(` of the call whose callee is the name at token `i` (`name(`, `name<T>(`, `name?.(`).
    fn call_open(&self, i: usize) -> Option<usize> {
        if !self.is_name(i) {
            return None;
        }
        let mut open = i + 1;
        if self.punct(open, "<") {
            open = self.skip_generics(open)?;
        }
        if self.punct(open, "?.") && self.punct(open + 1, "(") {
            open += 1;
        }
        if !self.punct(open, "(") {
            return None;
        }
        let close = self.pair[open]?;
        if self.ident_back(i, 1, "function") {
            return None;
        }
        if self.is_method_definition(close) && !self.is_member(i) {
            return None;
        }
        Some(open)
    }

    /// `foo(…) {` and `foo(…): T {` on one line: a method definition, not a call.
    fn is_method_definition(&self, close: usize) -> bool {
        let mut k = close + 1;
        if self.punct(k, ":") {
            match self.skip_type_to_brace(k + 1) {
                Some(b) => k = b,
                None => return false,
            }
        }
        self.punct(k, "{") && !self.t[k].newline
    }

    /// Token ranges of the arguments of the call whose `(` is at `open`.
    fn arguments(&self, open: usize, close: usize) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut start = open + 1;
        let mut i = start;
        while i < close {
            if self.punct(i, ",") {
                if i > start {
                    out.push((start, i));
                }
                start = i + 1;
            } else if self.t[i].kind == Kind::Punct
                && OPENERS.contains(&self.txt(i))
                && let Some(b) = self.pair[i].filter(|&b| b > i)
            {
                i = b;
            }
            i += 1;
        }
        if close > start {
            out.push((start, close));
        }
        out
    }

    /// The functions handling the page's `message` events: `window.onmessage = h`, `addEventListener('message', h)`.
    fn message_handlers(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for i in 0..self.t.len() {
            let found = if self.ident(i, "onmessage") && self.punct(i + 1, "=") {
                self.handler_at(i + 2, i, None)
            } else if self.t[i].kind == Kind::Ident && LISTENER_REGISTRATIONS.contains(&self.txt(i)) {
                self.call_open(i).and_then(|open| {
                    let args = self.arguments(open, self.pair[open]?);
                    let first = args.first().filter(|(a, b)| *b == a + 1).map(|(a, _)| &self.t[*a]);
                    (first?.constant()? == "message" && args.len() >= 2)
                        .then(|| self.handler_at(args[1].0, i, Some(args[1])))?
                })
            } else {
                None
            };
            if let Some(h) = found
                && !out.contains(&h)
            {
                out.push(h);
            }
        }
        out
    }

    fn in_message_handler(&self, k: usize) -> bool {
        self.message_handlers().iter().any(|&h| (self.fns[h].body_start..=self.fns[h].body_end).contains(&k))
    }

    /// The handler given at token `start`: a function written there, or a function named there.
    fn handler_at(&self, start: usize, p: usize, range: Option<(usize, usize)>) -> Option<usize> {
        if let Some(f) = self.fns.iter().position(|f| f.head == start) {
            return Some(f);
        }
        let single = range.is_none_or(|(a, b)| b == a + 1);
        if !single || !self.is_name(start) {
            return None;
        }
        let name = self.txt(start);
        (0..self.fns.len())
            .filter(|&f| self.fns[f].name.as_deref() == Some(name) && self.fns[f].container.is_none())
            .min_by_key(|&f| self.fns[f].head.abs_diff(p))
    }

    /// The pieces of the URL concatenation starting at token `i`.
    fn url(&self, i: usize) -> Vec<Piece> {
        let mut out = self.evaluate(std::slice::from_ref(&self.t[i]), i, 0);
        let mut j = i + 1;
        while self.punct(j, "+") && j + 1 < self.t.len() {
            let end = self.operand_end(j + 1);
            out.extend(self.evaluate(&self.t[j + 1..=end], i, 0));
            j = end + 1;
        }
        out
    }

    fn nui_url(&self, pieces: &[Piece], token: &Tok, src: &Source, f: &mut Facts) {
        // Characters with their source offsets (`None` when not written as is), markers for the values.
        let mut chars: Vec<(char, Option<usize>)> = Vec::new();
        let mut params = Vec::new();
        for piece in pieces {
            match piece {
                Piece::Lit(text, offset) => {
                    for (k, c) in text.char_indices() {
                        chars.push((c, offset.map(|o| o + k)));
                    }
                }
                Piece::Resource => chars.push((RESOURCE_MARK, None)),
                Piece::Param(fi, index) => {
                    params.push((chars.len(), *fi, *index));
                    chars.push((PARAM_MARK, None));
                }
                Piece::Dynamic => chars.push((DYNAMIC_MARK, None)),
            }
        }
        let s: String = chars.iter().map(|(c, _)| *c).collect();
        let Some((host, (from, to))) = match_url(&s) else { return };
        let host = (host != RESOURCE_MARK.to_string()).then_some(host);
        if host.as_ref().is_some_and(|h| h.to_ascii_lowercase().starts_with("cfx-nui-")) {
            return;
        }
        // Byte range of the name in `s` to char indices.
        let ci = |b: usize| s[..b].chars().count();
        let (cf, ct) = (ci(from), ci(to));
        let name = &s[from..to];
        if name == PARAM_MARK.to_string() {
            if let Some(&(_, fi, index)) = params.iter().find(|(at, _, _)| *at == cf)
                && let Some(fname) = &self.fns[fi].name
                && host.is_none()
            {
                let w = Wrapper {
                    kind: WrapKind::Fetch,
                    name: fname.clone(),
                    container: self.fns[fi].container.clone(),
                    param: index,
                    field: None,
                };
                if !f.wrappers.contains(&w) {
                    f.wrappers.push(w);
                }
            }
            return;
        }
        if name.is_empty() || name.chars().any(|c| matches!(c, RESOURCE_MARK | PARAM_MARK | DYNAMIC_MARK)) {
            return;
        }
        let slice = &chars[cf..ct];
        let contiguous = slice.first().and_then(|c| c.1).is_some_and(|first| {
            let mut expect = first;
            slice.iter().all(|(c, o)| {
                let ok = *o == Some(expect);
                expect += c.len_utf8();
                ok
            })
        });
        let at = match slice.first().and_then(|c| c.1) {
            Some(first) if contiguous => At::new(src, first, name.len(), true, first + name.len()),
            _ => At::new(src, token.start, name.len(), false, token.end),
        };
        f.urls.push(Url { name: name.to_owned(), at, host });
    }

    /// The message names compared in `handler`: `switch (e.data.action) { case 'open': }`, `data.action === 'open'`.
    fn listeners(&self, handler: usize, src: &Source, f: &mut Facts) {
        let h = &self.fns[handler];
        for q in h.body_start..=h.body_end.min(self.t.len().saturating_sub(1)) {
            let tk = &self.t[q];
            if tk.kind == Kind::Punct && COMPARISONS.contains(&tk.text(self.text)) && q > 0 {
                let (Some(left), Some(right)) = (self.tok(q - 1), self.tok(q + 1)) else { continue };
                if right.constant().is_some() && left.kind != Kind::Str {
                    if let Some(field) = self.field_before(q - 1) {
                        listen(field, right, src, f);
                    }
                } else if left.constant().is_some() && right.kind != Kind::Str {
                    if let Some(field) = self.field_after(q + 1) {
                        listen(field, left, src, f);
                    }
                } else if let Some(w) =
                    self.hook_param(q + 1, q, handler).or_else(|| self.hook_param_before(q - 1, q, handler))
                    && !f.wrappers.contains(&w)
                {
                    // A hook: the listener compares the message to a parameter of the function registering it.
                    f.wrappers.push(w);
                }
            }
            if self.ident(q, "switch")
                && self.punct(q + 1, "(")
                && let Some(close) = self.pair[q + 1]
            {
                let field = if close > q + 2 { self.field_before(close - 1) } else { None };
                let body = close + 1;
                let (Some(field), true) = (field, self.punct(body, "{")) else { continue };
                let Some(end) = self.pair[body] else { continue };
                for r in body + 1..end {
                    if self.ident(r, "case")
                        && self.brace[r] == Some(body)
                        && self.punct(r + 2, ":")
                        && self.t[r + 1].constant().is_some()
                    {
                        listen(field.clone(), &self.t[r + 1], src, f);
                    }
                }
            }
        }
    }

    /// The field ending the member chain whose last token is `k`: `action` in `event.data.action`.
    fn field_before(&self, k: usize) -> Option<String> {
        let tk = self.tok(k).filter(|t| t.kind == Kind::Ident)?;
        let member = self.is_member(k);
        let mut start = k;
        while start >= 2 && self.is_member(start) && self.t[start - 2].kind == Kind::Ident {
            start -= 2;
        }
        // `typeof event.data === 'string'` compares a type.
        if self.ident_back(start, 1, "typeof") {
            return None;
        }
        let text = tk.text(self.text);
        (member || !KEYWORDS.contains(&text)).then(|| text.to_owned())
    }

    /// The field ending the member chain starting at `k`.
    fn field_after(&self, k: usize) -> Option<String> {
        if !self.is_name(k) && !self.ident(k, "this") {
            return None;
        }
        let mut j = k;
        while j + 2 < self.t.len()
            && (self.punct(j + 1, ".") || self.punct(j + 1, "?."))
            && self.t[j + 2].kind == Kind::Ident
        {
            j += 2;
        }
        if self.punct(j + 1, "(") || self.punct(j + 1, "[") {
            return None;
        }
        Some(self.txt(j).to_owned())
    }

    /// `other === name`: the parameter `k` after the comparison `q`.
    fn hook_param(&self, k: usize, q: usize, handler: usize) -> Option<Wrapper> {
        if !self.is_name(k) || [".", "?.", "(", "["].iter().any(|p| self.punct(k + 1, p)) {
            return None;
        }
        self.hook(self.txt(k), q, handler, self.compared_field_before(q - 1, q))
    }

    /// `name === other`: the parameter `k` before the comparison `q`.
    fn hook_param_before(&self, k: usize, q: usize, handler: usize) -> Option<Wrapper> {
        if !self.is_name(k) || self.is_member(k) {
            return None;
        }
        self.hook(self.txt(k), q, handler, self.compared_field_after(q + 1, q))
    }

    fn hook(&self, name: &str, q: usize, handler: usize, field: Option<String>) -> Option<Wrapper> {
        let (fi, index) = self.param_ref(name, q)?;
        let f = &self.fns[fi];
        if fi == handler || f.body_start > self.fns[handler].head {
            return None;
        }
        Some(Wrapper {
            kind: WrapKind::Listen,
            name: f.name.clone()?,
            container: f.container.clone(),
            param: index,
            field,
        })
    }

    /// The field the operand ending at `k` reads: `action` for `e.data.action`, or for a local holding it.
    fn compared_field_before(&self, k: usize, q: usize) -> Option<String> {
        let tk = self.tok(k).filter(|t| t.kind == Kind::Ident)?;
        if self.is_member(k) {
            return self.field_before(k);
        }
        self.local_field(tk.text(self.text), q).or_else(|| self.field_before(k))
    }

    fn compared_field_after(&self, k: usize, q: usize) -> Option<String> {
        if self.is_name(k)
            && !self.punct(k + 1, ".")
            && !self.punct(k + 1, "?.")
            && let Some(f) = self.local_field(self.txt(k), q)
        {
            return Some(f);
        }
        self.field_after(k)
    }

    /// The field a local `name` declared before `p` holds: `const { action: kind } = e.data`,
    /// `const { action } = e.data`, `const kind = e.data.action` give `action`.
    fn local_field(&self, name: &str, p: usize) -> Option<String> {
        let stop = p.saturating_sub(MAX_LOOKBACK);
        for q in (stop..p.min(self.t.len())).rev() {
            if !self.is_name(q) || self.txt(q) != name {
                continue;
            }
            if let Some(open) = self.brace[q]
                && let Some(close) = self.pair[open].filter(|&c| c > q)
                && self.punct(close + 1, "=")
            {
                if self.punct_back(q, 1, ":")
                    && q >= 3
                    && self.is_name(q - 2)
                    && (self.punct(q - 3, "{") || self.punct(q - 3, ","))
                {
                    return Some(self.txt(q - 2).to_owned());
                }
                if (self.punct_back(q, 1, "{") || self.punct_back(q, 1, ","))
                    && (self.punct(q + 1, ",") || self.punct(q + 1, "}") || self.punct(q + 1, "="))
                {
                    return Some(name.to_owned());
                }
            }
            if self.declared(q) {
                let (a, b) = self.initializer(name, q + 1)?;
                let init = &self.t[a..b];
                let chain = init.len() >= 3
                    && init.len() % 2 == 1
                    && init.iter().enumerate().all(|(i, t)| {
                        if i % 2 == 0 {
                            t.kind == Kind::Ident
                        } else {
                            t.is_punct(self.text, ".") || t.is_punct(self.text, "?.")
                        }
                    });
                return chain.then(|| init[init.len() - 1].text(self.text).to_owned());
            }
        }
        None
    }

    // ---- modules ------------------------------------------------------------------------------

    fn is_import(&self, i: usize) -> bool {
        self.ident(i, "import") && !self.punct(i + 1, "(") && !self.punct(i + 1, ".")
    }

    fn is_module(&self) -> bool {
        (0..self.t.len()).any(|i| {
            self.brace[i].is_none() && (self.ident(i, "export") || self.is_import(i)) && !self.punct_back(i, 1, ".")
        })
    }

    fn imports(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for i in 0..self.t.len() {
            if !self.is_import(i) || self.brace[i].is_some() || self.punct_back(i, 1, ".") {
                continue;
            }
            let mut j = i + 1;
            if self.ident(j, "type") && !self.punct(j + 1, ",") && !self.ident(j + 1, "from") {
                j += 1;
            }
            if self.is_name(j) && !self.ident(j, "from") {
                set_import(&mut out, self.txt(j).to_owned(), "default".into());
                j += 1;
                if self.punct(j, ",") {
                    j += 1;
                }
            }
            if self.punct(j, "*") && self.ident(j + 1, "as") && self.is_name(j + 2) {
                set_import(&mut out, self.txt(j + 2).to_owned(), "*".into());
            } else if self.punct(j, "{")
                && let Some(end) = self.pair[j].filter(|&e| e > j)
            {
                let mut k = j + 1;
                while k < end {
                    if self.ident(k, "type") && self.is_name(k + 1) {
                        k += 1;
                    }
                    let imported = self
                        .tok(k)
                        .filter(|t| matches!(t.kind, Kind::Ident | Kind::Str))
                        .map(|t| t.value().map_or_else(|| t.text(self.text).to_owned(), str::to_owned));
                    if let Some(imported) = imported {
                        if self.ident(k + 1, "as") && self.tok(k + 2).is_some_and(|t| t.kind == Kind::Ident) {
                            set_import(&mut out, self.txt(k + 2).to_owned(), imported);
                            k += 3;
                        } else {
                            set_import(&mut out, imported.clone(), imported);
                            k += 1;
                        }
                    }
                    while k < end && !self.punct(k, ",") {
                        k += 1;
                    }
                    k += 1;
                }
            }
        }
        out
    }
}

fn listen(field: String, value: &Tok, src: &Source, f: &mut Facts) {
    let Some(name) = value.constant().filter(|n| !n.is_empty()) else { return };
    let plain = value.constant_is_plain();
    let start = if plain { value.content_start() } else { value.start };
    if f.listens.iter().any(|l| l.at.start == start) {
        return;
    }
    f.listens.push(Listen { field, name: name.to_owned(), at: At::new(src, start, name.len(), plain, value.end) });
}

/// `pair` of each bracket token and the innermost `{` enclosing each token.
fn brackets(text: &str, t: &[Tok]) -> (Vec<Option<usize>>, Vec<Option<usize>>) {
    let mut pair = vec![None; t.len()];
    let mut brace = vec![None; t.len()];
    let mut stack: Vec<usize> = Vec::new();
    let mut braces: Vec<usize> = Vec::new();
    for (i, tk) in t.iter().enumerate() {
        if tk.kind != Kind::Punct {
            brace[i] = braces.last().copied();
            continue;
        }
        let s = tk.text(text);
        match s {
            "(" | "[" | "{" => {
                brace[i] = braces.last().copied();
                stack.push(i);
                if s == "{" {
                    braces.push(i);
                }
            }
            ")" | "]" | "}" => {
                let open = match s {
                    ")" => "(",
                    "]" => "[",
                    _ => "{",
                };
                if let Some(at) = stack.iter().rposition(|&o| t[o].text(text) == open) {
                    let o = stack[at];
                    pair[o] = Some(i);
                    pair[i] = Some(o);
                    for removed in stack.drain(at..) {
                        if let Some(p) = braces.iter().rposition(|&b| b == removed) {
                            braces.remove(p);
                        }
                    }
                }
                brace[i] = braces.last().copied();
            }
            _ => brace[i] = braces.last().copied(),
        }
    }
    (pair, brace)
}

fn ends_operand(text: &str, t: &Tok) -> bool {
    match t.kind {
        Kind::Punct => matches!(t.text(text), ")" | "]" | "}"),
        Kind::Ident => !KEYWORDS.contains(&t.text(text)) || t.text(text) == "this",
        _ => true,
    }
}

fn starts_statement(t: &Tok) -> bool {
    matches!(t.kind, Kind::Ident | Kind::Str | Kind::Template)
}

fn starts_url(tk: &Tok) -> bool {
    let start = match tk.kind {
        Kind::Str => tk.value(),
        Kind::Template => tk.template().and_then(|t| t.quasis.first()).map(|q| q.text.as_str()),
        _ => None,
    };
    start.is_some_and(|s| {
        let l = s.get(..8).unwrap_or(s).to_ascii_lowercase();
        l.starts_with("https://") || l.starts_with("http://")
    })
}

/// Length of a leading `http://` / `https://` (any case).
fn scheme(s: &str) -> Option<usize> {
    let l = s.get(..8).unwrap_or(s).to_ascii_lowercase();
    if l.starts_with("https://") {
        Some(8)
    } else if l.starts_with("http://") {
        Some(7)
    } else {
        None
    }
}

fn host_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// `https?://(\x01|[A-Za-z0-9_-]+)/([^/?#]*)([?#].*)?` matched on the whole of `s`: the host and the
/// byte range of the name.
fn match_url(s: &str) -> Option<(String, (usize, usize))> {
    let h = scheme(s)?;
    let rest = &s[h..];
    let host_len = if rest.starts_with(RESOURCE_MARK) {
        RESOURCE_MARK.len_utf8()
    } else {
        rest.find(|c: char| !host_char(c)).unwrap_or(rest.len())
    };
    if host_len == 0 || !rest[host_len..].starts_with('/') {
        return None;
    }
    let from = h + host_len + 1;
    let to = from + s[from..].find(['/', '?', '#']).unwrap_or(s.len() - from);
    if s[to..].starts_with('/') {
        return None;
    }
    Some((rest[..host_len].to_owned(), (from, to)))
}

/// A place of a web file naming a callback or a message, found from its text.
#[derive(Clone, Debug, PartialEq)]
pub enum Slot {
    /// The name of a callback URL; `host`: the resource it names, `None` for `${GetParentResourceName()}`.
    Url { host: Option<String> },
    /// A message name compared by a listener.
    Message,
    /// Argument `arg` of `receiver.callee(…)`.
    Argument { callee: String, receiver: Option<String>, arg: usize },
}

/// The slot at byte `offset` of a web file and the start of the typed prefix.
pub fn slot_at(text: &str, ext: &str, offset: usize, keys: &[String]) -> Option<(Slot, usize)> {
    let (from, to) = script_ranges(text, ext).into_iter().find(|(a, b)| *a <= offset && offset <= *b)?;
    let tokens = tokenize(text, from, to);
    let k = tokens.iter().position(|t| {
        matches!(t.kind, Kind::Str | Kind::Template) && offset > t.start && offset <= content_end(text, t)
    })?;
    let token = &tokens[k];
    let prefix = text.get(token.start + 1..offset)?;
    if let Some(h) = scheme(prefix) {
        let rest = &prefix[h..];
        let host_len =
            if rest.starts_with("${") { rest.find('}').map(|c| c + 1) } else { rest.find(|c: char| !host_char(c)) };
        if let Some(n) = host_len.filter(|&n| n > 0 && rest[n..].starts_with('/'))
            && !rest[n + 1..].contains(['/', '?', '#'])
        {
            let host = (!rest.starts_with('$')).then(|| rest[..n].to_owned());
            return Some((Slot::Url { host }, token.start + 1 + h + n + 1));
        }
    }
    if token.template().is_some_and(|t| !t.exprs.is_empty()) || prefix.contains('\n') {
        return None;
    }
    let start = token.start + 1;
    let (pair, _) = brackets(text, &tokens);
    let is = |i: usize, p: &str| tokens.get(i).is_some_and(|t| t.is_punct(text, p));
    let comparison =
        |i: usize| tokens.get(i).is_some_and(|t| t.kind == Kind::Punct && COMPARISONS.contains(&t.text(text)));
    let message = |field: &str| {
        (keys.iter().any(|key| key == field) && Analysis::new(text, &tokens).in_message_handler(k))
            .then_some((Slot::Message, start))
    };
    if k > 0 && tokens[k - 1].is_ident(text, "case") {
        return message(&switch_field(text, &tokens, &pair, k)?);
    }
    if k > 0 && comparison(k - 1) {
        let field = k.checked_sub(2).and_then(|f| tokens.get(f)).filter(|t| t.kind == Kind::Ident)?;
        return message(field.text(text));
    }
    if comparison(k + 1) {
        let mut j = k + 2;
        tokens.get(j).filter(|t| t.kind == Kind::Ident)?;
        while j + 2 < tokens.len() && (is(j + 1, ".") || is(j + 1, "?.")) && tokens[j + 2].kind == Kind::Ident {
            j += 2;
        }
        return message(tokens[j].text(text));
    }
    argument(text, &tokens, k).map(|s| (s, start))
}

fn content_end(text: &str, t: &Tok) -> usize {
    let quote = text.as_bytes().get(t.start);
    let closed = t.end > t.start + 1
        && t.end <= text.len()
        && text.as_bytes().get(t.end - 1) == quote
        && (t.kind != Kind::Str || t.value().is_some());
    if closed { t.end - 1 } else { t.end }
}

/// The field a `switch` compares, for a `case` value at `k`.
fn switch_field(text: &str, tokens: &[Tok], pair: &[Option<usize>], k: usize) -> Option<String> {
    for s in (0..k).rev() {
        if !tokens[s].is_ident(text, "switch") || !tokens.get(s + 1).is_some_and(|t| t.is_punct(text, "(")) {
            continue;
        }
        let Some(close) = pair[s + 1] else { continue };
        let body = close + 1;
        if !tokens.get(body).is_some_and(|t| t.is_punct(text, "{")) {
            continue;
        }
        if !(body..=pair[body].unwrap_or(tokens.len())).contains(&k) {
            continue;
        }
        return tokens.get(close - 1).filter(|t| t.kind == Kind::Ident).map(|t| t.text(text).to_owned());
    }
    None
}

/// The call token `k` is an argument of.
fn argument(text: &str, tokens: &[Tok], k: usize) -> Option<Slot> {
    let mut depth = 0usize;
    let mut commas = 0;
    let mut i = k;
    let open = loop {
        i = i.checked_sub(1)?;
        let t = &tokens[i];
        if t.kind != Kind::Punct {
            continue;
        }
        match t.text(text) {
            ")" | "]" | "}" => depth += 1,
            o @ ("(" | "[" | "{") => {
                if depth == 0 {
                    if o != "(" {
                        return None;
                    }
                    break i;
                }
                depth -= 1;
            }
            "," if depth == 0 => commas += 1,
            ";" if depth == 0 => return None,
            _ => {}
        }
    };
    // The callee: `name(`, `name<T>(`, `name?.(`.
    let mut c = open.checked_sub(1)?;
    if tokens[c].is_punct(text, "?.") {
        c = c.checked_sub(1)?;
    }
    if tokens[c].is_punct(text, ">") {
        let mut angle = 0usize;
        loop {
            if tokens[c].is_punct(text, ">") {
                angle += 1;
            }
            if tokens[c].is_punct(text, "<") {
                angle = angle.saturating_sub(1);
                if angle == 0 {
                    break;
                }
            }
            c = c.checked_sub(1)?;
        }
        c = c.checked_sub(1)?;
    }
    let callee = tokens.get(c).filter(|t| t.kind == Kind::Ident)?;
    let member = c >= 1 && (tokens[c - 1].is_punct(text, ".") || tokens[c - 1].is_punct(text, "?."));
    let receiver = member.then(|| {
        c.checked_sub(2)
            .and_then(|r| tokens.get(r))
            .filter(|t| t.kind == Kind::Ident)
            .map_or_else(|| "?".to_owned(), |t| t.text(text).to_owned())
    });
    Some(Slot::Argument { callee: callee.text(text).to_owned(), receiver, arg: commas })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_text(text: &str, ext: &str) -> Facts {
        scan(&Source::plain(text), ext)
    }

    fn wrappers(f: &Facts) -> Vec<String> {
        let mut out: Vec<String> = f
            .wrappers
            .iter()
            .map(|w| {
                let kind = if w.kind == WrapKind::Fetch { "FETCH" } else { "LISTEN" };
                let container = w.container.as_ref().map(|c| format!("{c}.")).unwrap_or_default();
                let field = w.field.as_ref().map(|f| format!("@{f}")).unwrap_or_default();
                format!("{kind} {container}{}#{}{field}", w.name, w.param)
            })
            .collect();
        out.sort();
        out
    }

    fn listens(f: &Facts) -> Vec<String> {
        let mut out: Vec<String> = f.listens.iter().map(|l| format!("{}={}", l.field, l.name)).collect();
        out.sort();
        out
    }

    #[test]
    fn a_fetch_nui_wrapper_building_the_url_from_the_resource_name() {
        let f = scan_text(
            "const nuiWindow = window as any\n\
             export async function fetchNui<R = unknown>(callbackName: string, body?: unknown): Promise<R> {\n\
               const resource = nuiWindow.GetParentResourceName?.() ?? 'shop'\n\
               const response = await fetch(`https://${resource}/${callbackName}`, { method: 'POST', body: JSON.stringify(body ?? {}) })\n\
               return (await response.json()) as R\n}",
            "ts",
        );
        assert_eq!(wrappers(&f), ["FETCH fetchNui#0"]);
        assert!(f.urls.is_empty());
        assert!(f.module);
    }

    #[test]
    fn an_arrow_wrapper_and_a_chain_through_another_function() {
        let f = scan_text(
            "const request = async <T,>(name: string, body: unknown): Promise<T> => {\n\
               const response = await fetch(`https://${GetParentResourceName()}/${name}`, { method: 'POST', body: JSON.stringify(body) })\n\
               return response.json()\n}\n\
             export function callLua(eventName: string, data?: unknown) {\n  return request(eventName, data)\n}",
            "ts",
        );
        assert_eq!(wrappers(&f), ["FETCH request#0"]);
        let mut fw: Vec<String> = f
            .forwards
            .iter()
            .filter(|w| w.callee == "request")
            .map(|w| format!("{}#{}->{}({})", w.name, w.param, w.callee, w.arg))
            .collect();
        fw.sort();
        assert_eq!(fw, ["callLua#0->request(0)", "callLua#1->request(1)"]);
    }

    #[test]
    fn callback_urls_written_directly() {
        let text = "fetch(`https://${GetParentResourceName()}/close`, { method: 'POST' })\n\
                    $.post('https://legacy_hud/toggle', JSON.stringify({}))\n\
                    $.post('https://' + GetParentResourceName() + '/exit', '{}')\n\
                    fetch('https://api.example.com/users')\n\
                    fetch('https://cfx-nui-my_res/assets/logo.png')";
        let f = scan_text(text, "js");
        let mut urls: Vec<String> =
            f.urls.iter().map(|u| format!("{}@{}", u.name, u.host.as_deref().unwrap_or("null"))).collect();
        urls.sort();
        assert_eq!(urls, ["close@null", "exit@null", "toggle@legacy_hud"]);
        for u in &f.urls {
            assert!(u.at.exact);
            assert_eq!(&text[u.at.start..u.at.start + u.at.len], u.name);
        }
        assert!(!f.module);
    }

    #[test]
    fn calls_with_constant_arguments_generics_and_receivers() {
        let f = scan_text(
            "const data = await fetchNui<ShopData>('getShopData', { id: 1 })\napi.post(\"buy\")\nnui?.send(`close`)\nprint(`not ${constant}`)",
            "ts",
        );
        let mut calls: Vec<String> = f
            .calls
            .iter()
            .map(|c| {
                format!(
                    "{}{}({})={}",
                    c.receiver.as_ref().map(|r| format!("{r}.")).unwrap_or_default(),
                    c.callee,
                    c.arg,
                    c.value
                )
            })
            .collect();
        calls.sort();
        assert_eq!(calls, ["api.post(0)=buy", "fetchNui(0)=getShopData", "nui.send(0)=close"]);
    }

    #[test]
    fn a_use_nui_event_hook_comparing_a_destructured_field() {
        let f = scan_text(
            "export function useNuiEvent<P = unknown>(messageName: string, onMessage: (payload: P) => void) {\n\
               const latest = useRef(onMessage)\n  useEffect(() => {\n    function receive(msg: MessageEvent) {\n\
                   const { action: kind, data: payload } = msg.data ?? {}\n      if (kind !== messageName) return\n\
                   latest.current(payload as P)\n    }\n    window.addEventListener('message', receive)\n\
                 return () => window.removeEventListener('message', receive)\n  }, [messageName])\n}",
            "tsx",
        );
        assert_eq!(wrappers(&f), ["LISTEN useNuiEvent#0@action"]);
    }

    #[test]
    fn the_field_a_hook_compares_to_its_parameter() {
        let f = scan_text(
            "export const onLua = (name: string, run: (data: unknown) => void) => {\n\
               window.addEventListener('message', (e) => {\n    const type = e.data.type\n    if (type === name) run(e.data)\n  })\n}\n\
             export function onShorthand(name: string, run: () => void) {\n\
               window.addEventListener('message', ({ data }) => {\n    const { action } = data\n    if (name == action) run()\n  })\n}\n\
             function waitForReply(id: number) {\n\
               return new Promise((resolve) => window.addEventListener('message', (e) => {\n    if (e.data.requestId === id) resolve(e.data)\n  }))\n}\n\
             function fromOrigin(origin: string) {\n  window.addEventListener('message', (e) => { if (e.origin !== origin) return })\n}",
            "ts",
        );
        assert_eq!(
            wrappers(&f),
            [
                "LISTEN fromOrigin#0@origin",
                "LISTEN onLua#0@type",
                "LISTEN onShorthand#0@action",
                "LISTEN waitForReply#0@requestId"
            ]
        );
    }

    #[test]
    fn a_hook_declared_in_a_vue_composable() {
        let f = scan_text(
            "export function useNuiMessage<T>(action: string, handler: (data: T) => void) {\n\
               const listener = (event: MessageEvent) => {\n\
                 const parsed = typeof event.data === 'string' ? JSON.parse(event.data) : event.data\n\
                 if (parsed?.action !== action) return\n    handler(parsed.data)\n  }\n\
               onMounted(() => window.addEventListener('message', listener))\n\
               onUnmounted(() => window.removeEventListener('message', listener))\n}",
            "ts",
        );
        assert_eq!(wrappers(&f), ["LISTEN useNuiMessage#0@action"]);
        assert!(f.listens.is_empty(), "typeof event.data === 'string' is no message name");
    }

    #[test]
    fn message_names_compared_by_listeners() {
        let f = scan_text(
            "window.addEventListener('message', (event) => {\n  switch (event.data.action) {\n    case 'open':\n\
                   show(event.data.items)\n      break\n    case \"close\": hide(); break\n  }\n\
               if (event.data.type === 'notify') notify(event.data)\n})\n\
             function onMessage(e) {\n  const item = e.data\n  if ('setVisible' == item.action) setVisible(item.visible)\n}\n\
             window.addEventListener('message', onMessage)\nif (state.action === 'not_a_listener') {}",
            "js",
        );
        assert_eq!(listens(&f), ["action=close", "action=open", "action=setVisible", "type=notify"]);
    }

    #[test]
    fn a_jquery_listener_with_a_destructured_switch() {
        let f = scan_text(
            "$(function () {\n  window.onmessage = function (event) {\n    const { action, data } = event.data\n\
                 switch (action) {\n      case 'ui': toggle(data); break\n    }\n  }\n})",
            "js",
        );
        assert_eq!(listens(&f), ["action=ui"]);
    }

    #[test]
    fn class_methods_and_object_literal_wrappers_keep_their_container() {
        let f = scan_text(
            "export class Nui {\n  static async post<T>(endpoint: string, body?: unknown): Promise<T> {\n\
                 const res = await fetch(`https://${GetParentResourceName()}/${endpoint}`, { method: 'POST', body: JSON.stringify(body) })\n\
                 return res.json()\n  }\n}\n\
             export const api = {\n  call(name, payload) {\n\
                 return fetch(`https://${GetParentResourceName()}/${name}`, { method: 'POST', body: JSON.stringify(payload) })\n  },\n}",
            "ts",
        );
        assert_eq!(wrappers(&f), ["FETCH Nui.post#0", "FETCH api.call#0"]);
    }

    #[test]
    fn imports_and_registrations_of_javascript_client_scripts() {
        let f = scan_text(
            "import fetchNui, { useNuiEvent as onNui, type Foo } from './nui'\nimport * as bridge from '@/bridge'\n\
             RegisterNuiCallbackType('close')\non('__cfx_nui:close', (data, cb) => cb({}))\nRegisterNuiCallback(name, cb)",
            "ts",
        );
        let imports: Vec<(&str, &str)> = f.imports.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        assert_eq!(imports, [("fetchNui", "default"), ("onNui", "useNuiEvent"), ("Foo", "Foo"), ("bridge", "*")]);
        assert_eq!(f.registrations.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["close", "close"]);
        assert_eq!(f.dynamic, 1);
    }

    #[test]
    fn vue_single_file_components_are_read_through_their_scripts() {
        let text = "<template>\n  <button @click=\"close\">Don't close</button>\n</template>\n<script setup lang=\"ts\">\n\
                    import { fetchNui } from '@/utils/fetchNui'\nconst close = () => fetchNui('closeMenu')\n</script>";
        let f = scan_text(text, "vue");
        assert_eq!(f.calls.len(), 1);
        let c = &f.calls[0];
        assert_eq!(c.value, "closeMenu");
        assert_eq!(&text[c.at.start..c.at.start + c.at.len], "closeMenu");
        assert_eq!(f.imports, [("fetchNui".to_owned(), "fetchNui".to_owned())]);
        assert_eq!(script_ranges("<SCRIPT src=x></Script >a<script>b", "html"), [(14, 14), (33, 34)]);
    }

    #[test]
    fn strings_comments_regexes_and_templates_do_not_confuse_the_analysis() {
        let f = scan_text(
            "// fetchNui('commented')\n/* fetch('https://res/commented') */\nconst re = /fetchNui\\('x'\\)/g\nconst ratio = a / b / c\n\
             const s = \"it's fetchNui('quoted')\"\nconst t = `${nested(`inner ${deep}`)} fetchNui('template')`\nfetchNui('real')",
            "ts",
        );
        assert_eq!(f.calls.iter().map(|c| c.value.as_str()).collect::<Vec<_>>(), ["real"]);
    }

    #[test]
    fn generated_bundles_are_skipped() {
        assert!(is_generated("index.min.js", "a"));
        assert!(is_generated("index-4f2a.js", &"x".repeat(5_000)));
        assert!(!is_generated("App.tsx", &"const a = 1\n".repeat(500)));
    }

    fn slot(text: &str) -> Option<Slot> {
        let keys = ["action".to_owned(), "type".to_owned()];
        let offset = text.find('|').unwrap();
        slot_at(&text.replace('|', ""), "ts", offset, &keys).map(|(s, _)| s)
    }

    #[test]
    fn completion_slots_found_from_the_text() {
        assert_eq!(
            slot("fetchNui<Data>('ge|', {})"),
            Some(Slot::Argument { callee: "fetchNui".into(), receiver: None, arg: 0 })
        );
        assert_eq!(
            slot("api.post(x, 'b|')"),
            Some(Slot::Argument { callee: "post".into(), receiver: Some("api".into()), arg: 1 })
        );
        assert_eq!(slot("fetch(`https://${GetParentResourceName()}/cl|`)"), Some(Slot::Url { host: None }));
        assert_eq!(slot("fetch('https://other_res/cl|')"), Some(Slot::Url { host: Some("other_res".into()) }));
        assert_eq!(
            slot("window.addEventListener('message', (event) => { switch (event.data.action) { case 'op|': break } })"),
            Some(Slot::Message)
        );
        assert_eq!(
            slot("window.onmessage = function (e) { const data = e.data; if (data.type === '|') {} }"),
            Some(Slot::Message)
        );
        assert_eq!(
            slot(
                "function onMessage(e) { if ('|' == e.data.action) {} }\nwindow.addEventListener('message', onMessage)"
            ),
            Some(Slot::Message)
        );
        assert_eq!(slot("window.addEventListener('message', (e) => { if (e.data.other === '|') {} })"), None);
        let text = "fetchNui<Data>('ge', {})";
        assert_eq!(slot_at(text, "ts", 17, &[]).map(|(_, start)| start), Some(16), "the prefix starts after the quote");
        let url = "fetch('https://shop/cl')";
        assert_eq!(slot_at(url, "js", 22, &[]).map(|(_, start)| &url[start..22]), Some("cl"));
    }

    #[test]
    fn strings_compared_outside_of_a_listener_are_no_message_slots() {
        assert_eq!(
            slot("function reducer(state: State, action: Action) { switch (action.type) { case '|': return state } }"),
            None
        );
        assert_eq!(slot("const isText = (input: HTMLInputElement) => input.type === '|'"), None);
        assert_eq!(slot("if ('|' === item.action) {}"), None);
        assert_eq!(slot("const s = 'x|'"), None);
    }
}
