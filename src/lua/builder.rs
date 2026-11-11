use super::token::{T, Tokens};
use super::tree::{N, NONE, NodeData};
use std::borrow::Cow;

/// Which white space and comments around a node belong to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bind {
    /// The edge sits after the run: a node starts at its first token.
    After,
    /// The edge sits before the run: a node ends at its last token.
    Before,
    /// A documentation comment right above the node belongs to it.
    DocAbove,
    /// A documentation comment at the end of the node's last line belongs to it.
    DocBehind,
}

#[derive(Clone, Copy)]
enum Event {
    Start { kind: N, lexeme: u32, before: u32, bind: Bind },
    Done { start: u32, lexeme: u32, bind: Bind, message: u32 },
    Error { lexeme: u32, message: u32 },
    Dropped,
}

#[derive(Clone, Copy)]
pub struct Marker(u32);

#[derive(Clone, Copy)]
pub struct Done {
    start: u32,
    done: u32,
}

pub struct Builder<'a> {
    src: &'a str,
    tokens: &'a Tokens,
    kinds: &'a [T],
    cur: usize,
    limit: usize,
    events: Vec<Event>,
    messages: Vec<Cow<'static, str>>,
}

impl<'a> Builder<'a> {
    pub fn new(src: &'a str, tokens: &'a Tokens) -> Self {
        let kinds = tokens.kinds();
        Self {
            src,
            tokens,
            kinds,
            cur: 0,
            limit: kinds.len(),
            events: Vec::with_capacity(kinds.len()),
            messages: Vec::new(),
        }
    }

    fn skip(&mut self) {
        while self.cur < self.limit && self.kinds[self.cur].is_trivia() {
            self.cur += 1;
        }
    }

    /// The current token, white space and comments skipped; `None` at the end of the file or of the block.
    pub fn tok(&mut self) -> Option<T> {
        self.skip();
        (self.cur < self.limit).then(|| self.kinds[self.cur])
    }

    pub fn at(&mut self, kind: T) -> bool {
        self.tok() == Some(kind)
    }

    pub fn eof(&mut self) -> bool {
        self.tok().is_none()
    }

    pub fn advance(&mut self) {
        if !self.eof() {
            self.cur += 1;
        }
    }

    pub fn eat(&mut self, kind: T) -> bool {
        let found = self.at(kind);
        if found {
            self.cur += 1;
        }
        found
    }

    /// The token `steps` tokens after the current one, white space and comments skipped.
    pub fn look_ahead(&mut self, steps: usize) -> Option<T> {
        self.skip();
        let mut at = self.cur;
        for _ in 0..steps {
            at += 1;
            while at < self.limit && self.kinds[at].is_trivia() {
                at += 1;
            }
        }
        (at < self.limit).then(|| self.kinds[at])
    }

    /// The token `steps` tokens away from the current position, white space and comments counted.
    pub fn raw(&self, steps: isize) -> Option<T> {
        let at = self.cur as isize + steps;
        (at >= 0 && (at as usize) < self.limit).then(|| self.kinds[at as usize])
    }

    pub fn token_text(&mut self) -> &'a str {
        self.skip();
        if self.cur < self.limit { self.tokens.text(self.src, self.cur) } else { "" }
    }

    pub fn offset(&mut self) -> usize {
        self.skip();
        if self.cur < self.kinds.len() { self.tokens.start(self.cur) } else { self.src.len() }
    }

    pub fn source(&self) -> &'a str {
        self.src
    }

    pub fn position(&self) -> usize {
        self.cur
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Moves back to `position` and reads no further than `limit`: the tokens of one block.
    pub fn enter(&mut self, position: usize, limit: usize) {
        self.cur = position;
        self.limit = limit;
    }

    pub fn mark(&mut self) -> Marker {
        self.mark_bound(Bind::After)
    }

    pub fn mark_bound(&mut self, bind: Bind) -> Marker {
        if !self.events.is_empty() {
            self.skip();
        }
        self.events.push(Event::Start { kind: N::Error, lexeme: self.cur as u32, before: NONE, bind });
        Marker(self.events.len() as u32 - 1)
    }

    fn close(&mut self, marker: Marker, kind: N, message: u32) -> Done {
        if let Event::Start { kind: slot, .. } = &mut self.events[marker.0 as usize] {
            *slot = kind;
        }
        self.events.push(Event::Done { start: marker.0, lexeme: self.cur as u32, bind: Bind::Before, message });
        Done { start: marker.0, done: self.events.len() as u32 - 1 }
    }

    pub fn done(&mut self, marker: Marker, kind: N) -> Done {
        self.close(marker, kind, NONE)
    }

    /// Closes `marker` as an error element; an empty one stays against the token before it.
    pub fn fail(&mut self, marker: Marker, message: impl Into<Cow<'static, str>>) -> Done {
        if let Event::Start { lexeme, bind, .. } = &mut self.events[marker.0 as usize]
            && self.kinds[*lexeme as usize..self.cur].iter().all(|kind| kind.is_trivia())
        {
            *bind = Bind::Before;
        }
        self.messages.push(message.into());
        self.close(marker, N::Error, self.messages.len() as u32 - 1)
    }

    /// An empty error element at the current position; a second one at the same place is not added.
    pub fn error(&mut self, message: impl Into<Cow<'static, str>>) {
        let last = self.events.iter().rev().find(|event| !matches!(event, Event::Dropped));
        if let Some(Event::Error { lexeme, .. }) = last
            && *lexeme as usize == self.cur
        {
            return;
        }
        self.messages.push(message.into());
        self.events.push(Event::Error { lexeme: self.cur as u32, message: self.messages.len() as u32 - 1 });
    }

    /// A new marker that starts where `done` starts and wraps it.
    pub fn precede(&mut self, done: Done) -> Marker {
        let Event::Start { lexeme, .. } = self.events[done.start as usize] else { unreachable!("not a marker") };
        let at = self.events.len() as u32;
        self.events.push(Event::Start { kind: N::Error, lexeme, before: NONE, bind: Bind::After });
        if let Event::Start { before, .. } = &mut self.events[done.start as usize] {
            debug_assert_eq!(*before, NONE, "a marker is preceded once");
            *before = at;
        }
        Marker(at)
    }

    /// Forgets what was read since `marker` was set, the marker included.
    pub fn rollback(&mut self, marker: Marker) {
        if let Event::Start { lexeme, .. } = self.events[marker.0 as usize] {
            self.cur = lexeme as usize;
        }
        self.events.truncate(marker.0 as usize);
    }

    pub fn drop(&mut self, marker: Marker) {
        self.events[marker.0 as usize] = Event::Dropped;
    }

    pub fn drop_done(&mut self, done: Done) {
        self.events[done.start as usize] = Event::Dropped;
        self.events[done.done as usize] = Event::Dropped;
    }

    pub fn bind(&mut self, done: Done, left: Option<Bind>, right: Option<Bind>) {
        if let (Some(left), Event::Start { bind, .. }) = (left, &mut self.events[done.start as usize]) {
            *bind = left;
        }
        if let (Some(right), Event::Done { bind, .. }) = (right, &mut self.events[done.done as usize]) {
            *bind = right;
        }
    }

    /// The kind of the node that was closed last.
    pub fn latest_done(&self) -> Option<N> {
        self.events.iter().rev().find_map(|event| match event {
            Event::Done { start, .. } => match self.events[*start as usize] {
                Event::Start { kind, .. } => Some(kind),
                _ => None,
            },
            _ => None,
        })
    }

    pub fn finish(self) -> (Vec<NodeData>, Vec<(u32, Box<str>)>) {
        let Builder { src, tokens, kinds, mut events, messages, .. } = self;
        let mut items = linear(&mut events);
        balance(&mut items, src, tokens);
        let mut nodes: Vec<NodeData> = Vec::with_capacity(items.len() / 2 + 1);
        let mut errors: Vec<(u32, Box<str>)> = Vec::new();
        let mut open: Vec<u32> = Vec::new();
        for item in &items {
            let parent = open.last().copied().unwrap_or(NONE);
            let id = nodes.len() as u32;
            match item.what {
                What::Start(kind) => {
                    nodes.push(NodeData { kind, parent, first: item.lexeme, end: item.lexeme, next: id + 1 });
                    open.push(id);
                }
                What::Done(message) => {
                    let Some(closed) = open.pop() else { continue };
                    nodes[closed as usize].end = item.lexeme;
                    nodes[closed as usize].next = id;
                    if message != NONE {
                        errors.push((closed, messages[message as usize].as_ref().into()));
                    }
                }
                What::Error(message) => {
                    nodes.push(NodeData { kind: N::Error, parent, first: item.lexeme, end: item.lexeme, next: id + 1 });
                    errors.push((id, messages[message as usize].as_ref().into()));
                }
            }
        }
        if let Some(root) = nodes.first_mut() {
            root.first = 0;
            root.end = kinds.len() as u32;
        }
        errors.sort_by_key(|(id, _)| *id);
        (nodes, errors)
    }
}

#[derive(Clone, Copy)]
enum What {
    Start(N),
    Done(u32),
    Error(u32),
}

#[derive(Clone, Copy)]
struct Item {
    what: What,
    lexeme: u32,
    bind: Bind,
}

/// The events in tree order: a marker that precedes another one comes out before it.
fn linear(events: &mut [Event]) -> Vec<Item> {
    let mut items = Vec::with_capacity(events.len());
    let mut chain: Vec<(N, u32, Bind)> = Vec::new();
    for i in 0..events.len() {
        match std::mem::replace(&mut events[i], Event::Dropped) {
            Event::Start { kind, lexeme, before, bind } => {
                chain.push((kind, lexeme, bind));
                let mut next = before;
                while next != NONE {
                    let Event::Start { kind, lexeme, before, bind } =
                        std::mem::replace(&mut events[next as usize], Event::Dropped)
                    else {
                        break;
                    };
                    chain.push((kind, lexeme, bind));
                    next = before;
                }
                for (kind, lexeme, bind) in chain.drain(..).rev() {
                    items.push(Item { what: What::Start(kind), lexeme, bind });
                }
            }
            Event::Done { lexeme, bind, message, .. } => items.push(Item { what: What::Done(message), lexeme, bind }),
            Event::Error { lexeme, message } => {
                items.push(Item { what: What::Error(message), lexeme, bind: Bind::Before })
            }
            Event::Dropped => {}
        }
    }
    items
}

fn line_breaks(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut count = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => count += 1,
            b'\r' => {
                if bytes.get(i + 1) == Some(&b'\n') {
                    i += 1;
                }
                count += 1;
            }
            _ => {}
        }
        i += 1;
    }
    count
}

/// Where the edge of a node falls inside the run of white space and comments `from..to`.
fn edge(bind: Bind, src: &str, tokens: &Tokens, from: usize, to: usize) -> usize {
    match bind {
        Bind::After => to,
        Bind::Before => from,
        Bind::DocAbove => {
            let mut lines = 0;
            for i in (from..to).rev() {
                if tokens.kind(i) == T::DocComment {
                    return i;
                }
                lines += line_breaks(tokens.text(src, i));
                if lines > 1 {
                    break;
                }
            }
            to
        }
        Bind::DocBehind => {
            for i in from..to {
                if tokens.kind(i) == T::DocComment {
                    return i + 1;
                }
                if tokens.text(src, i).contains('\n') {
                    break;
                }
            }
            from
        }
    }
}

/// Moves each edge over the white space and comments around it, as IntelliJ's builder does.
fn balance(items: &mut [Item], src: &str, tokens: &Tokens) {
    let count = tokens.len();
    let mut last = 0usize;
    for i in 1..items.len().saturating_sub(1) {
        let previous = items[i - 1].lexeme as usize;
        let mut lexeme = items[i].lexeme as usize;
        let mut from = lexeme.max(last);
        while from > previous && tokens.kind(from - 1).is_trivia() {
            from -= 1;
        }
        let mut to = lexeme;
        while to < count && tokens.kind(to).is_trivia() {
            to += 1;
        }
        if from != to {
            lexeme = if from < to { edge(items[i].bind, src, tokens, from, to) } else { from };
        } else if lexeme < from {
            lexeme = from;
        }
        items[i].lexeme = lexeme as u32;
        last = lexeme;
    }
}
