use super::lexer::{Tok, Token, coarse};
use super::scope::Scopes;
use super::token::{Tokens, lex};
use super::tree::{Node, Tree};
use crate::text::LineIndex;
use std::any::Any;
use std::cell::{OnceCell, RefCell};
use std::rc::Rc;

pub struct Source {
    pub text: String,
    pub all: Vec<Token>,
    pub toks: Vec<Token>,
    pub pair: Vec<Option<usize>>,
    pub lines: LineIndex,
    fine: Option<Rc<Tokens>>,
    code: Vec<u32>,
    tree: OnceCell<Tree>,
    scopes: OnceCell<Scopes>,
    models: RefCell<Vec<Rc<dyn Any>>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Seg {
    Field { name: String, tok: usize, safe: bool },
    Method { name: String, tok: usize },
    Index { open: usize, close: Option<usize> },
    Call { open: usize, close: Option<usize>, args: Vec<(usize, usize)> },
    StrCall { tok: usize },
    TableCall { open: usize, close: Option<usize> },
}

#[derive(Clone, Debug)]
pub struct Chain {
    pub start: usize,
    pub head: String,
    pub segs: Vec<Seg>,
    pub end: usize,
}

impl Chain {
    pub fn path(&self, n: usize) -> String {
        let mut out = self.head.clone();
        for s in &self.segs[..n.min(self.segs.len())] {
            match s {
                Seg::Field { name, .. } => {
                    out.push('.');
                    out.push_str(name);
                }
                Seg::Method { name, .. } => {
                    out.push(':');
                    out.push_str(name);
                }
                Seg::Index { .. } => out.push_str("[]"),
                Seg::Call { .. } | Seg::StrCall { .. } | Seg::TableCall { .. } => out.push_str("()"),
            }
        }
        out
    }
}

#[derive(Clone, Debug)]
pub struct Call {
    pub callee: String,
    pub name_tok: usize,
    pub open: usize,
    pub close: Option<usize>,
    pub args: Vec<(usize, usize)>,
    pub chain: usize,
    pub seg: usize,
}

impl Source {
    pub fn new(text: &str) -> Self {
        let fine = lex(text);
        let (all, held) = coarse(text, &fine);
        let mut toks = Vec::with_capacity(all.len());
        let mut code_of = Vec::with_capacity(all.len() + 1);
        for token in &all {
            code_of.push(toks.len() as u32);
            if !token.is_comment() {
                toks.push(*token);
            }
        }
        code_of.push(toks.len() as u32);
        let code = held.iter().map(|at| code_of[*at as usize]).collect();
        let pair = match_brackets(text, &toks);
        Self {
            text: text.to_owned(),
            lines: LineIndex::new(text),
            all,
            toks,
            pair,
            fine: Some(Rc::new(fine)),
            code,
            tree: OnceCell::new(),
            scopes: OnceCell::new(),
            models: RefCell::default(),
        }
    }

    pub fn plain(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            lines: LineIndex::new(text),
            all: Vec::new(),
            toks: Vec::new(),
            pair: Vec::new(),
            fine: None,
            code: Vec::new(),
            tree: OnceCell::new(),
            scopes: OnceCell::new(),
            models: RefCell::default(),
        }
    }

    /// The syntax tree, parsed once; a source that is not Lua has an empty one.
    pub fn tree(&self) -> &Tree {
        self.tree.get_or_init(|| match &self.fine {
            Some(fine) => Tree::of(&self.text, fine.clone()),
            None => Tree::parse(""),
        })
    }

    pub fn root(&self) -> Node<'_> {
        let text = if self.fine.is_some() { self.text.as_str() } else { "" };
        self.tree().root(text)
    }

    /// The declaration each name refers to, resolved once.
    pub fn scopes(&self) -> &Scopes {
        self.scopes.get_or_init(|| Scopes::new(self.root()))
    }

    /// What a reader derives from this text, built once and kept as long as the text.
    pub fn model<M: 'static>(&self, build: impl FnOnce(&Source) -> M) -> Rc<M> {
        let kept = self.models.borrow().iter().find(|model| model.is::<M>()).cloned();
        if let Some(model) = kept.and_then(|model| model.downcast::<M>().ok()) {
            return model;
        }
        let model = Rc::new(build(self));
        self.models.borrow_mut().push(model.clone());
        model
    }

    /// The index in `toks` of the tree token `index`: the token itself, or the code token after it
    /// when it is white space or a comment.
    pub fn tok_of(&self, index: usize) -> usize {
        self.code.get(index).map_or(self.toks.len(), |at| *at as usize)
    }

    pub fn tok_text(&self, i: usize) -> &str {
        self.toks.get(i).map_or("", |t| t.text(&self.text))
    }

    pub fn is_op(&self, i: usize, op: &str) -> bool {
        self.toks.get(i).is_some_and(|t| t.is_op(&self.text, op))
    }

    /// The `?` of safe navigation, followed by `.`, `:`, `[` or call arguments (spaces allowed).
    pub fn is_safe_nav(&self, i: usize) -> bool {
        self.is_op(i, "?")
            && self.toks.get(i + 1).is_some_and(|t| match t.kind {
                Tok::Op => matches!(t.text(&self.text), "." | ":" | "[" | "(" | "{"),
                Tok::Str { .. } | Tok::LongStr | Tok::Hash { .. } => true,
                _ => false,
            })
    }

    pub fn is_kw(&self, i: usize, kw: &str) -> bool {
        self.toks.get(i).is_some_and(|t| t.is_kw(&self.text, kw))
    }

    pub fn is_name(&self, i: usize) -> bool {
        self.toks.get(i).is_some_and(|t| t.kind == Tok::Name)
    }

    pub fn string_at(&self, i: usize) -> Option<String> {
        let t = self.toks.get(i)?;
        t.is_string().then(|| t.string_value(&self.text))
    }

    pub fn string_arg(&self, arg: (usize, usize)) -> Option<usize> {
        (arg.1 == arg.0 + 1 && self.toks[arg.0].is_string()).then_some(arg.0)
    }

    pub fn tok_before(&self, at: usize) -> Option<usize> {
        let i = self.toks.partition_point(|t| t.start < at);
        if i == 0 { None } else { Some(i - 1) }
    }

    pub fn tok_at(&self, at: usize) -> Option<usize> {
        let i = self.tok_before(at)?;
        (self.toks[i].end >= at).then_some(i)
    }

    pub fn in_comment(&self, at: usize) -> bool {
        let i = self.all.partition_point(|t| t.start < at);
        i > 0 && {
            let t = self.all[i - 1];
            t.is_comment() && at <= t.end && (at < t.end || t.kind == Tok::LineComment)
        }
    }

    pub fn span(&self, a: usize, b: usize) -> (usize, usize) {
        if a >= b || a >= self.toks.len() {
            let at = self.toks.get(a).map_or(self.text.len(), |t| t.start);
            return (at, at);
        }
        (self.toks[a].start, self.toks[b - 1].end)
    }

    pub fn chains(&self) -> Vec<Chain> {
        let mut out = Vec::new();
        for i in 0..self.toks.len() {
            let t = self.toks[i];
            if t.kind != Tok::Name {
                continue;
            }
            // Not a chain start when it continues one (`a.b`, `a:b`, `a?.b`), or names a field in a table constructor.
            if i > 0 {
                let p = self.toks[i - 1];
                if p.kind == Tok::Op && matches!(p.text(&self.text), "." | ":" | "::") {
                    continue;
                }
            }
            out.push(self.chain_from(i));
        }
        out
    }

    fn chain_from(&self, start: usize) -> Chain {
        let head = self.tok_text(start).to_owned();
        let mut segs = Vec::new();
        let mut i = start + 1;
        while i < self.toks.len() {
            let t = self.toks[i];
            let text = t.text(&self.text);
            match (t.kind, text) {
                (Tok::Op, "?") if self.is_safe_nav(i) => i += 1,
                (Tok::Op, ".") if self.is_name(i + 1) => {
                    let safe = self.is_op(i - 1, "?");
                    segs.push(Seg::Field { name: self.tok_text(i + 1).to_owned(), tok: i + 1, safe });
                    i += 2;
                }
                (Tok::Op, ":") if self.is_name(i + 1) => {
                    segs.push(Seg::Method { name: self.tok_text(i + 1).to_owned(), tok: i + 1 });
                    i += 2;
                }
                (Tok::Op, "[") => {
                    let close = self.pair[i];
                    segs.push(Seg::Index { open: i, close });
                    match close {
                        Some(c) => i = c + 1,
                        None => break,
                    }
                }
                (Tok::Op, "(") => {
                    // A `(` on the next line after a complete statement is ambiguous in
                    // Lua; treat it as a call like the reference parser does.
                    let close = self.pair[i];
                    let args = self.split_args(i + 1, close.unwrap_or(self.toks.len()));
                    segs.push(Seg::Call { open: i, close, args });
                    match close {
                        Some(c) => i = c + 1,
                        None => break,
                    }
                }
                (Tok::Str { .. } | Tok::LongStr, _) => {
                    segs.push(Seg::StrCall { tok: i });
                    i += 1;
                }
                (Tok::Op, "{") => {
                    let close = self.pair[i];
                    segs.push(Seg::TableCall { open: i, close });
                    match close {
                        Some(c) => i = c + 1,
                        None => break,
                    }
                }
                _ => break,
            }
        }
        Chain { start, head, segs, end: i }
    }

    pub fn split_args(&self, from: usize, to: usize) -> Vec<(usize, usize)> {
        let mut args = Vec::new();
        if from >= to {
            return args;
        }
        let mut s = from;
        let mut i = from;
        // Blocks of function bodies passed as arguments: their commas (`for k, v in …`) are not separators.
        let mut blocks = 0usize;
        while i < to {
            let t = self.toks[i];
            if t.kind == Tok::Keyword {
                match t.text(&self.text) {
                    "function" | "if" | "do" | "repeat" => blocks += 1,
                    "end" | "until" => blocks = blocks.saturating_sub(1),
                    _ => {}
                }
            }
            if blocks > 0 {
                i += 1;
                continue;
            }
            if t.kind == Tok::Op {
                match t.text(&self.text) {
                    "(" | "[" | "{" => {
                        if let Some(c) = self.pair[i] {
                            i = c.min(to);
                        } else {
                            i = to;
                            continue;
                        }
                    }
                    "," => {
                        args.push((s, i));
                        s = i + 1;
                    }
                    _ => {}
                }
            }
            i += 1;
        }
        args.push((s, to));
        args
    }

    pub fn calls(&self, chains: &[Chain]) -> Vec<Call> {
        let mut out = Vec::new();
        for (ci, c) in chains.iter().enumerate() {
            // `function a.b(x)` declares, it does not call.
            if c.start > 0 && self.is_kw(c.start - 1, "function") {
                continue;
            }
            for (si, s) in c.segs.iter().enumerate() {
                let (open, close, args) = match s {
                    Seg::Call { open, close, args } => (*open, *close, args.clone()),
                    Seg::StrCall { tok } => (*tok, Some(*tok), vec![(*tok, *tok + 1)]),
                    Seg::TableCall { open, close } => {
                        (*open, *close, vec![(*open, close.map_or(*open + 1, |c| c + 1))])
                    }
                    _ => continue,
                };
                let name_tok = match si.checked_sub(1).map(|p| &c.segs[p]) {
                    Some(Seg::Field { tok, .. } | Seg::Method { tok, .. }) => *tok,
                    Some(_) => continue,
                    None => c.start,
                };
                out.push(Call { callee: c.path(si), name_tok, open, close, args, chain: ci, seg: si });
            }
        }
        out
    }

    pub fn arg_at(&self, call: &Call, at: usize) -> Option<(usize, (usize, usize))> {
        let open = self.toks[call.open];
        let close_at = call.close.map_or(self.text.len(), |c| self.toks[c].start);
        if at <= open.start || at > close_at {
            // A sugar call's string is its own argument.
            if call.close == Some(call.open) && at > open.start && at <= open.end {
                return Some((0, call.args[0]));
            }
            return None;
        }
        let mut index = 0;
        for (n, arg) in call.args.iter().enumerate() {
            let next_start = call.args.get(n + 1).map_or(close_at, |a| {
                // The comma before the next argument.
                self.toks.get(a.0.saturating_sub(1)).map_or(close_at, |t| t.start)
            });
            index = n;
            if at <= next_start {
                return Some((n, *arg));
            }
        }
        call.args.get(index).map(|a| (index, *a))
    }

    pub fn is_statement_start(&self, i: usize) -> bool {
        if i == 0 {
            return true;
        }
        let p = self.toks[i - 1];
        let ptext = p.text(&self.text);
        match p.kind {
            Tok::Keyword => {
                matches!(
                    ptext,
                    "do" | "then" | "else" | "end" | "repeat" | "break" | "return" | "nil" | "true" | "false"
                )
            }
            Tok::Op => matches!(ptext, ";" | ")" | "]" | "}"),
            Tok::Name | Tok::Number | Tok::Str { .. } | Tok::LongStr | Tok::Hash { .. } => true,
            _ => true,
        }
    }
}

fn match_brackets(src: &str, toks: &[Token]) -> Vec<Option<usize>> {
    let mut pair = vec![None; toks.len()];
    let mut stack: Vec<(usize, u8)> = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        if t.kind != Tok::Op {
            continue;
        }
        let close = match t.text(src) {
            "(" => {
                stack.push((i, b')'));
                continue;
            }
            "[" => {
                stack.push((i, b']'));
                continue;
            }
            "{" => {
                stack.push((i, b'}'));
                continue;
            }
            ")" => b')',
            "]" => b']',
            "}" => b'}',
            _ => continue,
        };
        // Unbalanced code: unwind to the nearest opener of this kind.
        if let Some(pos) = stack.iter().rposition(|(_, c)| *c == close) {
            let (open, _) = stack[pos];
            stack.truncate(pos);
            pair[open] = Some(i);
            pair[i] = Some(open);
        }
    }
    pair
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_is_built_once_and_kept_with_its_source() {
        let s = Source::new("local a = 1");
        let built = std::cell::Cell::new(0);
        let count = |src: &Source| {
            built.set(built.get() + 1);
            src.toks.len()
        };
        let first = s.model(count);
        let again = s.model(count);
        assert!(Rc::ptr_eq(&first, &again));
        assert_eq!((built.get(), *again), (1, 4));
        assert_eq!(*s.model(|src| src.text.clone()), "local a = 1", "one model per type");
        assert_eq!(*Source::new("local a").model(count), 2, "and per source");
    }

    #[test]
    fn chains_and_calls() {
        let src = "Player(src).state:set('job', job, true)\nCitizen.CreateThread(function() Wait(0) end)";
        let s = Source::new(src);
        let chains = s.chains();
        let calls = s.calls(&chains);
        let callees: Vec<&str> = calls.iter().map(|c| c.callee.as_str()).collect();
        assert_eq!(callees, ["Player", "Player().state:set", "Citizen.CreateThread", "Wait"]);
        let set = &calls[1];
        assert_eq!(set.args.len(), 3);
        assert_eq!(s.string_at(s.string_arg(set.args[0]).unwrap()).as_deref(), Some("job"));
    }

    #[test]
    fn safe_navigation_allows_spaces_after_the_question_mark() {
        let s = Source::new("local j = LocalPlayer ? . state ? : get('job')\ncb?[[text]]\nlocal y = x ? 1 : 2");
        let chains = s.chains();
        let calls = s.calls(&chains);
        let callees: Vec<&str> = calls.iter().map(|c| c.callee.as_str()).collect();
        assert_eq!(callees, ["LocalPlayer.state:get", "cb"]);
        let player = chains.iter().find(|c| c.head == "LocalPlayer").unwrap();
        assert!(matches!(&player.segs[0], Seg::Field { name, safe: true, .. } if name == "state"));
        assert!(!s.is_safe_nav(s.toks.iter().rposition(|t| t.is_op(&s.text, "?")).unwrap()), "`x ? 1` is not");
    }

    #[test]
    fn commas_inside_function_arguments_do_not_split() {
        let src = "RegisterNUICallback('x', function(d, cb) for _, v in pairs(d) do if v then cb(v, 1) end end end, 3)";
        let s = Source::new(src);
        let calls = s.calls(&s.chains());
        assert_eq!(calls[0].args.len(), 3);
        assert_eq!(s.tok_text(calls[0].args[2].0), "3");
    }

    #[test]
    fn caret_argument() {
        let src = "TriggerServerEvent('shop:buy', item, )";
        let s = Source::new(src);
        let chains = s.chains();
        let calls = s.calls(&chains);
        let at = src.find("shop").unwrap();
        assert_eq!(s.arg_at(&calls[0], at).unwrap().0, 0);
        let at = src.find(", )").unwrap() + 2;
        assert_eq!(s.arg_at(&calls[0], at).unwrap().0, 2);
    }

    #[test]
    fn sugar_calls_and_unclosed() {
        let src = "RegisterNetEvent 'esx:playerLoaded'\nAddEventHandler('x', function(";
        let s = Source::new(src);
        let calls = s.calls(&s.chains());
        assert_eq!(calls[0].callee, "RegisterNetEvent");
        assert_eq!(s.string_at(s.string_arg(calls[0].args[0]).unwrap()).as_deref(), Some("esx:playerLoaded"));
        assert_eq!(calls[1].callee, "AddEventHandler");
        assert!(calls[1].close.is_none());
    }
}
