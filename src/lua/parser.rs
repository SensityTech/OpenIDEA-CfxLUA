use super::builder::{Bind, Builder, Done, Marker};
use super::token::{T, Tokens, lex};
use super::tree::{N, Tree};
use std::rc::Rc;

pub const INCREMENT_NOT_SUPPORTED: &str = "'++' is not supported in CfxLua, use 'x += 1'";
pub const NESTED_TOO_DEEPLY: &str = "Code is nested too deeply to be analysed";

const MAX_DEPTH: usize = 200;

const STATEMENT_START: &[T] = &[
    T::Do,
    T::If,
    T::While,
    T::Repeat,
    T::DoubleColon,
    T::Goto,
    T::Break,
    T::Return,
    T::Local,
    T::For,
    T::Function,
    T::Semi,
    T::Id,
    T::LParen,
];

const UNARY_PRIORITY: u8 = 12;

/// Left and right priorities of a binary operator, as the Lua parser has them: `..` and `^` are
/// right associative and `^` binds tighter than a unary operator on its left.
fn binary_priority(kind: T) -> Option<(u8, u8)> {
    Some(match kind {
        T::Or => (1, 1),
        T::And => (2, 2),
        T::Gt | T::Lt | T::Ge | T::Le | T::Ne | T::Eq => (3, 3),
        T::BitOr => (4, 4),
        T::BitTilde => (5, 5),
        T::BitAnd => (6, 6),
        T::BitLtLt | T::BitRtRt => (7, 7),
        T::Concat => (9, 8),
        T::Plus | T::Minus => (10, 10),
        T::Mult | T::Div | T::DoubleDiv | T::Mod => (11, 11),
        T::Exp => (14, 13),
        _ => return None,
    })
}

/// For the keyword that opens a construct: the keywords to step over, and those that close it.
/// The block after `then` stops at `else` and `elseif` only when it is the block being delimited.
fn closers(begin: T, outer: bool) -> Option<(&'static [T], &'static [T])> {
    Some(match begin {
        T::Do | T::Else | T::Function => (&[], &[T::End]),
        T::Repeat => (&[], &[T::Until]),
        T::If => (&[T::Then, T::Else, T::ElseIf], &[T::End]),
        T::Then if outer => (&[], &[T::Else, T::ElseIf, T::End]),
        T::Then => (&[T::Else, T::ElseIf], &[T::End]),
        _ => return None,
    })
}

impl Tree {
    pub fn parse(src: &str) -> Tree {
        Tree::of(src, Rc::new(lex(src)))
    }

    /// The tree of `src`, whose tokens are `tokens`.
    pub fn of(src: &str, tokens: Rc<Tokens>) -> Tree {
        let (nodes, errors) = {
            let mut parser = Parser { b: Builder::new(src, &tokens), depth: 0, brackets: 0 };
            parser.file();
            parser.b.finish()
        };
        Tree { tokens, nodes, errors }
    }
}

struct Parser<'a> {
    b: Builder<'a>,
    depth: usize,
    /// The brackets open around the current position, inside the block being parsed.
    brackets: usize,
}

impl Parser<'_> {
    fn file(&mut self) {
        let m = self.b.mark();
        if self.b.at(T::Shebang) {
            let shebang = self.b.mark();
            self.b.advance();
            if !self.b.eat(T::ShebangContent) && self.b.raw(0).is_none() {
                self.b.error("shebang content expected");
            }
            self.b.done(shebang, N::ShebangLine);
        }
        self.statements();
        self.b.done(m, N::File);
    }

    fn statements(&mut self) {
        while self.statement().is_some() {}
    }

    fn expect(&mut self, kind: T, message: &'static str) -> bool {
        let found = self.b.eat(kind);
        if !found {
            self.b.error(message);
        }
        found
    }

    fn error_token(&mut self, message: &'static str) -> Done {
        let m = self.b.mark();
        self.b.advance();
        self.b.fail(m, message)
    }

    /// `++` written as two adjacent `+`: CfxLua has no increment operator.
    fn is_increment(&mut self) -> bool {
        self.b.at(T::Plus) && self.b.raw(1) == Some(T::Plus)
    }

    /// `//=`, `%=` and `..=`, which CfxLua does not have: the operator glued to `=`.
    fn is_missing_compound(&mut self) -> bool {
        matches!(self.b.tok(), Some(T::DoubleDiv | T::Mod | T::Concat)) && self.b.raw(1) == Some(T::Assign)
    }

    fn error_increment(&mut self) -> Done {
        let m = self.b.mark();
        self.b.advance();
        self.b.advance();
        self.b.fail(m, INCREMENT_NOT_SUPPORTED)
    }

    /// Statements and table fields take the documentation comment above them, some the one behind.
    fn finish(&mut self, m: Marker, kind: N) -> Done {
        let done = self.b.done(m, kind);
        let behind = matches!(kind, N::LocalDef | N::AssignStat | N::TableField).then_some(Bind::DocBehind);
        let above = (kind != N::EmptyStat).then_some(Bind::DocAbove);
        self.b.bind(done, above, behind);
        done
    }

    fn finish_stat(&mut self, m: Marker, kind: N) -> Done {
        self.b.eat(T::Semi);
        self.finish(m, kind)
    }

    fn statement(&mut self) -> Option<Done> {
        let parsed = match self.b.tok() {
            Some(T::Do) => Some(self.do_stat()),
            Some(T::If) => Some(self.if_stat()),
            Some(T::While) => Some(self.while_stat()),
            Some(T::Repeat) => Some(self.repeat_stat()),
            Some(T::DoubleColon) => Some(self.label_stat()),
            Some(T::Goto) => Some(self.goto_stat()),
            Some(T::Break) => Some(self.break_stat()),
            Some(T::Return) => Some(self.return_stat()),
            Some(T::Local) => Some(self.local_stat()),
            Some(T::For) => Some(self.for_stat()),
            Some(T::Function) => Some(self.function_stat()),
            Some(T::Semi) => Some(self.empty_stat()),
            _ => self.other_stat(),
        };
        parsed.or_else(|| self.recover())
    }

    /// A token that starts no statement is reported and skipped with what follows it, up to the next
    /// statement start: one bad statement never breaks the rest of the file.
    fn recover(&mut self) -> Option<Done> {
        if self.b.eof() {
            return None;
        }
        if self.is_increment() {
            return Some(self.prefix_increment());
        }
        let m = self.b.mark();
        let text = self.b.token_text();
        self.b.advance();
        while let Some(kind) = self.b.tok() {
            if STATEMENT_START.contains(&kind) || self.is_increment() {
                break;
            }
            self.b.advance();
        }
        Some(self.b.fail(m, format!("Unexpected symbol '{text}'")))
    }

    /// `++x`: the operand written on the same line stays inside the statement.
    fn prefix_increment(&mut self) -> Done {
        let m = self.b.mark();
        let operator_end = self.b.offset() + 2;
        let error = self.error_increment();
        let next = self.b.offset();
        let same_line = !self.b.source()[operator_end..next].contains(['\n', '\r']);
        if same_line && self.suffixed(true).is_some() {
            return self.finish_stat(m, N::ExprStat);
        }
        self.b.drop(m);
        error
    }

    fn do_stat(&mut self) -> Done {
        let m = self.b.mark();
        self.b.advance();
        self.block();
        self.expect(T::End, "'end' expected");
        self.finish_stat(m, N::DoStat)
    }

    fn if_stat(&mut self) -> Done {
        let m = self.b.mark();
        self.b.advance();
        self.expect_expr();
        self.expect(T::Then, "'then' expected");
        self.block();
        while self.b.eat(T::ElseIf) {
            self.expect_expr();
            self.expect(T::Then, "'then' expected");
            self.block();
        }
        if self.b.eat(T::Else) {
            self.block();
        }
        self.expect(T::End, "'end' expected");
        self.finish_stat(m, N::IfStat)
    }

    fn while_stat(&mut self) -> Done {
        let m = self.b.mark();
        self.b.advance();
        self.expect_expr();
        self.expect(T::Do, "'do' expected");
        self.block();
        self.expect(T::End, "'end' expected");
        self.finish_stat(m, N::WhileStat)
    }

    fn repeat_stat(&mut self) -> Done {
        let m = self.b.mark();
        self.b.advance();
        self.block();
        self.expect(T::Until, "'until' expected");
        self.expect_expr();
        self.finish_stat(m, N::RepeatStat)
    }

    fn label_stat(&mut self) -> Done {
        let m = self.b.mark();
        self.b.advance();
        self.expect(T::Id, "ID expected");
        self.expect(T::DoubleColon, "'::' expected");
        self.finish_stat(m, N::LabelStat)
    }

    fn goto_stat(&mut self) -> Done {
        let m = self.b.mark();
        self.b.advance();
        self.expect(T::Id, "ID expected");
        self.finish_stat(m, N::GotoStat)
    }

    fn break_stat(&mut self) -> Done {
        let m = self.b.mark();
        self.b.advance();
        self.finish_stat(m, N::BreakStat)
    }

    fn return_stat(&mut self) -> Done {
        let m = self.b.mark();
        self.b.advance();
        self.expect_expr_list(false);
        self.finish_stat(m, N::ReturnStat)
    }

    fn empty_stat(&mut self) -> Done {
        let m = self.b.mark();
        while self.b.eat(T::Semi) {}
        self.finish(m, N::EmptyStat)
    }

    fn for_stat(&mut self) -> Done {
        let m = self.b.mark();
        self.b.advance();
        let names = self.param_names();
        let kind = if names > 1 || self.b.at(T::In) {
            self.expect(T::In, "'in' expected");
            self.expect_expr_list(true);
            N::ForBStat
        } else {
            self.expect(T::Assign, "'=' expected");
            self.expect_expr();
            // Without its comma the limit is not read: what follows is the next statement.
            if self.expect(T::Comma, "',' expected") {
                self.expect_expr();
                if self.b.eat(T::Comma) {
                    self.expect_expr();
                }
            }
            N::ForAStat
        };
        self.expect(T::Do, "'do' expected");
        self.block();
        self.expect(T::End, "'end' expected");
        self.finish_stat(m, kind)
    }

    fn param_name(&mut self, required: bool) -> bool {
        if self.b.at(T::Id) {
            let m = self.b.mark();
            self.b.advance();
            self.b.done(m, N::ParamNameDef);
            return true;
        }
        if required {
            self.b.error("ID expected");
        }
        false
    }

    fn param_names(&mut self) -> usize {
        if !self.param_name(true) {
            return 0;
        }
        let mut count = 1;
        while self.b.eat(T::Comma) {
            self.param_name(true);
            count += 1;
        }
        count
    }

    fn local_stat(&mut self) -> Done {
        let m = self.b.mark();
        if self.b.look_ahead(1) == Some(T::Function) {
            self.b.advance();
            self.b.advance();
            self.expect(T::Id, "ID expected");
            self.func_body();
            return self.finish_stat(m, N::LocalFuncDef);
        }
        self.b.advance();
        self.name_list();
        if self.b.eat(T::Assign) {
            let list = self.b.mark();
            if self.expr_list().is_none() {
                self.b.error("Expression expected");
            }
            self.b.done(list, N::ExprList);
        } else if self.b.eat(T::In) {
            self.in_source();
        }
        self.finish_stat(m, N::LocalDef)
    }

    fn attribute(&mut self) {
        if !self.b.at(T::Lt) {
            return;
        }
        let m = self.b.mark();
        self.b.advance();
        self.expect(T::Id, "ID expected");
        self.expect(T::Gt, "> expected");
        self.b.done(m, N::Attribute);
    }

    fn name_list(&mut self) {
        let first = self.b.mark();
        if !self.expect(T::Id, "ID expected") {
            self.b.done(first, N::NameList);
            return;
        }
        let first = self.b.done(first, N::NameDef);
        self.attribute();
        while self.b.eat(T::Comma) {
            let name = self.b.mark();
            if self.expect(T::Id, "ID expected") {
                self.b.done(name, N::NameDef);
                self.attribute();
            } else {
                self.b.drop(name);
            }
        }
        let list = self.b.precede(first);
        self.b.done(list, N::NameList);
    }

    /// The source of a CfxLua `in` unpacking: one suffixed expression, a direct child of the statement.
    fn in_source(&mut self) {
        if matches!(self.b.tok(), Some(T::Id | T::LParen)) {
            self.suffixed(true);
        } else {
            self.b.error("Name or '(' expected, the source of 'in' must be a suffixed expression");
            self.expr();
        }
        if self.b.at(T::Comma) {
            let extra = self.b.mark();
            while self.b.eat(T::Comma) {
                self.expr();
            }
            self.b.fail(extra, "Only one expression is allowed after 'in'");
        }
    }

    fn func_body(&mut self) -> Done {
        let m = self.b.mark();
        if !self.b.eat(T::LParen) {
            return self.b.fail(m, "'(' expected");
        }
        let named = self.param_name(false);
        while named && self.b.eat(T::Comma) {
            if !self.param_name(false) && !self.b.eat(T::Ellipsis) {
                self.b.error("ID or '...' expected");
            }
        }
        if !named {
            self.b.eat(T::Ellipsis);
        }
        self.expect(T::RParen, "')' expected");
        self.block();
        self.expect(T::End, "'end' expected");
        self.b.done(m, N::FuncBody)
    }

    fn function_stat(&mut self) -> Done {
        let m = self.b.mark();
        self.b.advance();
        let mut kind = N::FuncDef;
        if !self.b.at(T::Id) {
            self.b.error("ID expected");
        } else if !matches!(self.b.look_ahead(1), Some(T::Dot | T::Colon)) {
            self.b.advance();
        } else {
            kind = N::ClassMethodDef;
            let name = self.b.mark();
            self.b.advance();
            let mut path = self.b.done(name, N::NameExpr);
            while matches!(self.b.tok(), Some(T::Dot | T::Colon)) {
                self.b.advance();
                self.expect(T::Id, "ID expected");
                if !matches!(self.b.tok(), Some(T::Dot | T::Colon)) {
                    break;
                }
                let index = self.b.precede(path);
                path = self.b.done(index, N::IndexExpr);
            }
            let method = self.b.precede(path);
            self.b.done(method, N::ClassMethodName);
        }
        self.func_body();
        self.finish_stat(m, kind)
    }

    fn other_stat(&mut self) -> Option<Done> {
        let expr = self.expr()?;
        if matches!(self.b.latest_done(), Some(N::NameExpr | N::IndexExpr)) {
            if self.b.tok().is_some_and(T::is_compound_assign) {
                self.b.advance();
                self.expect_expr();
                let m = self.b.precede(expr);
                return Some(self.finish_stat(m, N::CompoundAssignStat));
            }
            if self.is_missing_compound() {
                let op = self.b.token_text();
                let m = self.b.mark();
                self.b.advance();
                self.b.advance();
                self.b.fail(m, format!("'{op}=' is not supported in CfxLua, use 'x = x {op} y'"));
                self.expect_expr();
                let m = self.b.precede(expr);
                return Some(self.finish_stat(m, N::CompoundAssignStat));
            }
            let mut targets = false;
            while self.b.eat(T::Comma) {
                targets = true;
                // Only the first target may use safe navigation.
                if self.suffixed(false).is_none() {
                    self.b.error("Expression expected");
                }
            }
            if targets || matches!(self.b.tok(), Some(T::Assign | T::In)) {
                let list = self.b.precede(expr);
                let list = self.b.done(list, N::VarList);
                if self.b.eat(T::Assign) {
                    self.expect_expr_list(true);
                } else if self.b.eat(T::In) {
                    self.in_source();
                } else if self.b.tok().is_some_and(T::is_compound_assign) {
                    self.error_token("'=' expected");
                    self.expect_expr_list(true);
                } else {
                    self.b.error("'=' expected");
                }
                let m = self.b.precede(list);
                return Some(self.finish_stat(m, N::AssignStat));
            }
        }
        let m = self.b.precede(expr);
        if self.is_increment() {
            self.error_increment();
        }
        Some(self.finish_stat(m, N::ExprStat))
    }

    /// The statements between an opening keyword and the keyword that closes it. The end is found by
    /// balancing keywords first, so a broken statement inside never eats the closing keyword.
    fn block(&mut self) {
        let mut back = -1;
        let begin = loop {
            match self.b.raw(back) {
                Some(T::Whitespace) => back -= 1,
                Some(kind) => break kind,
                None => return,
            }
        };
        let m = self.b.mark_bound(Bind::Before);
        let begin = if begin == T::RParen { T::Function } else { begin };
        let start = self.b.position();
        let outer = self.b.limit();
        let brackets = std::mem::take(&mut self.brackets);
        self.skip_block(begin, brackets > 0);
        let end = self.b.position();
        self.b.enter(start, end);
        if self.depth >= MAX_DEPTH {
            self.swallow();
        } else {
            self.depth += 1;
            self.statements();
            self.depth -= 1;
        }
        self.brackets = brackets;
        self.b.enter(end, outer);
        let done = self.b.done(m, N::Block);
        self.b.bind(done, None, Some(Bind::After));
    }

    fn swallow(&mut self) -> Option<Done> {
        if self.b.eof() {
            return None;
        }
        let m = self.b.mark();
        while !self.b.eof() {
            self.b.advance();
        }
        Some(self.b.fail(m, NESTED_TOO_DEEPLY))
    }

    /// Moves to the keyword that closes the block opened by `begin`, or to the end of the input.
    /// Each keyword that opens a construct on the way is matched with the one that closes it.
    /// A closing bracket ends what was left open inside its pair; one that closes nothing in the
    /// block ends the block itself when `bounded`: it was written between brackets and lacks its `end`.
    fn skip_block(&mut self, begin: T, bounded: bool) {
        let Some((skips, ends)) = closers(begin, true) else { return };
        let mut open = vec![(skips, ends, 0usize)];
        let mut brackets: Vec<T> = Vec::new();
        while let (Some(kind), Some((skips, ends, _))) = (self.b.tok(), open.last().copied()) {
            let outer = open.len() == 1;
            let opener = match kind {
                T::RParen => Some(T::LParen),
                T::RBrack => Some(T::LBrack),
                T::RCurly => Some(T::LCurly),
                _ => None,
            };
            if let Some(opener) = opener {
                match brackets.iter().rposition(|bracket| *bracket == opener) {
                    Some(at) => {
                        brackets.truncate(at);
                        while open.len() > 1 && open.last().is_some_and(|(_, _, inside)| *inside > at) {
                            open.pop();
                        }
                    }
                    None if bounded => return,
                    None => {}
                }
            } else if matches!(kind, T::LParen | T::LBrack | T::LCurly) {
                brackets.push(kind);
            } else if skips.contains(&kind) {
            } else if ends.contains(&kind) {
                if outer {
                    return;
                }
                open.pop();
            } else if kind == T::Until {
                if outer {
                    return;
                }
                open.pop();
                continue;
            } else if let Some((skips, ends)) = closers(kind, false) {
                open.push((skips, ends, brackets.len()));
            }
            self.b.advance();
        }
    }

    fn bracketed(&mut self, parse: fn(&mut Self) -> Option<Done>) -> Option<Done> {
        self.brackets += 1;
        let parsed = parse(self);
        self.brackets -= 1;
        parsed
    }

    fn expect_expr(&mut self) -> Option<Done> {
        let expr = self.expr();
        if expr.is_none() {
            self.b.error("Expression expected");
        }
        expr
    }

    fn expr_list(&mut self) -> Option<Done> {
        let first = self.expr()?;
        while self.b.eat(T::Comma) {
            self.expect_expr();
        }
        Some(first)
    }

    /// The arguments of a call. One that is missing before a comma is reported and the list goes
    /// on: the arguments after the one being typed stay arguments.
    fn arguments(&mut self) -> Option<Done> {
        let mut first = None;
        loop {
            let argument = self.expr();
            first = first.or(argument);
            if argument.is_none() && (first.is_some() || self.b.at(T::Comma)) {
                self.b.error("Expression expected");
            }
            if !self.b.eat(T::Comma) {
                return first;
            }
        }
    }

    fn expect_expr_list(&mut self, required: bool) -> Option<Done> {
        match self.expr_list() {
            Some(first) => {
                let list = self.b.precede(first);
                Some(self.b.done(list, N::ExprList))
            }
            None => {
                if required {
                    self.b.error("Expression expected");
                }
                None
            }
        }
    }

    fn expr(&mut self) -> Option<Done> {
        self.subexpr(0)
    }

    fn skip_group(&mut self) {
        let mut depth = 0usize;
        while let Some(kind) = self.b.tok() {
            self.b.advance();
            match kind {
                T::LParen | T::LBrack | T::LCurly => depth += 1,
                T::RParen | T::RBrack | T::RCurly => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    /// At the nesting limit the rest of the expression is stepped over, a bracketed group or a
    /// function as one piece each: the statements that follow are still parsed.
    fn too_deep(&mut self) -> Option<Done> {
        let m = self.b.mark();
        let start = self.b.position();
        loop {
            while matches!(self.b.tok(), Some(T::Not | T::GetN | T::Minus | T::BitTilde)) {
                self.b.advance();
            }
            match self.b.tok() {
                Some(T::LParen | T::LCurly) => self.skip_group(),
                Some(T::Function) => {
                    self.b.advance();
                    self.skip_block(T::Function, false);
                    self.b.eat(T::End);
                }
                Some(T::Id | T::Number | T::String | T::HashString | T::Nil | T::True | T::False | T::Ellipsis) => {
                    self.b.advance()
                }
                _ => break,
            }
            loop {
                match self.b.tok() {
                    Some(T::Dot | T::Colon) => {
                        self.b.advance();
                        self.b.eat(T::Id);
                    }
                    Some(T::Question | T::String | T::HashString) => self.b.advance(),
                    Some(T::LParen | T::LBrack | T::LCurly) => self.skip_group(),
                    _ => break,
                }
            }
            let operator = self.b.tok().and_then(binary_priority).is_some();
            if !operator || self.is_increment() || self.is_missing_compound() {
                break;
            }
            self.b.advance();
        }
        if self.b.position() == start {
            self.b.drop(m);
            return None;
        }
        Some(self.b.fail(m, NESTED_TOO_DEEPLY))
    }

    fn missing_operand(&mut self) {
        let m = self.b.mark();
        self.b.fail(m, "Expression expected");
    }

    fn nested(&mut self, limit: u8) -> Option<Done> {
        if self.depth >= MAX_DEPTH {
            return self.too_deep();
        }
        self.depth += 1;
        let expr = self.subexpr(limit);
        self.depth -= 1;
        expr
    }

    /// An operand and the binary operators that bind tighter than `limit`.
    fn subexpr(&mut self, limit: u8) -> Option<Done> {
        if self.depth >= MAX_DEPTH {
            return self.too_deep();
        }
        let mut left = if matches!(self.b.tok(), Some(T::Not | T::GetN | T::Minus | T::BitTilde)) {
            let m = self.b.mark();
            let op = self.b.mark();
            self.b.advance();
            self.b.done(op, N::UnaryOp);
            if self.nested(UNARY_PRIORITY).is_none() {
                self.missing_operand();
            }
            self.b.done(m, N::UnaryExpr)
        } else {
            self.depth += 1;
            let value = self.primary(true).or_else(|| self.closure());
            self.depth -= 1;
            value?
        };
        // After an operator that lacks its right operand, only looser operators carry on.
        let mut ceiling = u8::MAX;
        while let Some((left_priority, right_priority)) = self.b.tok().and_then(binary_priority) {
            // `x++` is not an addition and `x //= 1` not a division: the statement parser reports them.
            if left_priority <= limit || left_priority >= ceiling || self.is_increment() || self.is_missing_compound() {
                break;
            }
            let op = self.b.mark();
            self.b.advance();
            self.b.done(op, N::BinaryOp);
            let right = self.nested(right_priority);
            if right.is_none() {
                self.missing_operand();
                ceiling = left_priority;
            }
            let m = self.b.precede(left);
            left = self.b.done(m, N::BinaryExpr);
        }
        Some(left)
    }

    fn closure(&mut self) -> Option<Done> {
        if !self.b.at(T::Function) {
            return None;
        }
        let m = self.b.mark();
        self.b.advance();
        self.func_body();
        Some(self.b.done(m, N::ClosureExpr))
    }

    /// CfxLua `suffixedexp`: a primary expression and its suffixes, each one optionally after the safe
    /// navigation `?`. `safe_nav` is false for the targets of an assignment other than the first.
    fn suffixed(&mut self, safe_nav: bool) -> Option<Done> {
        self.primary(safe_nav)
    }

    fn primary(&mut self, safe_nav: bool) -> Option<Done> {
        let (mut prefix, callable) = self.prefix()?;
        // After `:name` the call arguments must follow directly.
        let mut after_method = false;
        loop {
            if self.b.at(T::Question) {
                let next = self.b.look_ahead(1);
                let index = matches!(next, Some(T::Dot | T::Colon | T::LBrack));
                let call = callable && matches!(next, Some(T::LParen | T::String | T::LCurly | T::HashString));
                if !index && !call {
                    self.error_token("expected suffixed expression after '?'");
                    break;
                }
                if after_method {
                    self.error_token("'?' is not allowed between a method name and its arguments");
                } else if !safe_nav {
                    self.error_token("'?' is only allowed in the first target of an assignment");
                } else {
                    self.b.advance();
                }
            }
            let method = self.b.at(T::Colon);
            let mut suffix = self.index_suffix(prefix);
            if suffix.is_none() && callable {
                suffix = self.call_suffix(prefix);
            }
            match suffix {
                Some(suffix) => prefix = suffix,
                None => break,
            }
            after_method = method;
        }
        Some(prefix)
    }

    fn index_suffix(&mut self, prefix: Done) -> Option<Done> {
        match self.b.tok() {
            Some(T::Dot | T::Colon) => {
                self.b.advance();
                self.expect(T::Id, "ID expected");
            }
            Some(T::LBrack) => {
                self.b.advance();
                self.bracketed(Self::expect_expr);
                self.expect(T::RBrack, "']' expected");
            }
            _ => return None,
        }
        let m = self.b.precede(prefix);
        Some(self.b.done(m, N::IndexExpr))
    }

    fn call_suffix(&mut self, prefix: Done) -> Option<Done> {
        match self.b.tok() {
            Some(T::LParen) => {
                let args = self.b.mark();
                self.b.advance();
                self.bracketed(Self::arguments);
                self.expect(T::RParen, "')' expected");
                self.b.done(args, N::ListArgs);
            }
            Some(T::String | T::HashString) => {
                let literal = self.b.mark();
                self.b.advance();
                let literal = self.b.done(literal, N::LiteralExpr);
                let arg = self.b.precede(literal);
                self.b.done(arg, N::SingleArg);
            }
            Some(T::LCurly) => {
                if let Some(table) = self.table() {
                    let arg = self.b.precede(table);
                    self.b.done(arg, N::SingleArg);
                }
            }
            _ => return None,
        }
        let m = self.b.precede(prefix);
        Some(self.b.done(m, N::CallExpr))
    }

    /// The expression a suffix chain starts with, and whether call arguments may follow it.
    fn prefix(&mut self) -> Option<(Done, bool)> {
        match self.b.tok()? {
            T::LParen => {
                let m = self.b.mark();
                self.b.advance();
                self.bracketed(Self::expect_expr);
                self.expect(T::RParen, "')' expected");
                Some((self.b.done(m, N::ParenExpr), true))
            }
            T::Id => {
                let m = self.b.mark();
                self.b.advance();
                Some((self.b.done(m, N::NameExpr), true))
            }
            T::Number | T::String | T::HashString | T::Nil | T::True | T::False | T::Ellipsis => {
                let m = self.b.mark();
                self.b.advance();
                Some((self.b.done(m, N::LiteralExpr), false))
            }
            T::LCurly => self.table().map(|table| (table, false)),
            _ => None,
        }
    }

    fn table(&mut self) -> Option<Done> {
        if !self.b.at(T::LCurly) {
            return None;
        }
        let m = self.b.mark();
        self.b.advance();
        self.brackets += 1;
        if self.table_field().is_some() {
            loop {
                let missing = if matches!(self.b.tok(), Some(T::Semi | T::Comma)) {
                    let sep = self.b.mark();
                    self.b.advance();
                    self.b.done(sep, N::TableFieldSep);
                    None
                } else {
                    let m = self.b.mark();
                    Some(self.b.fail(m, ", or ; expected"))
                };
                if self.table_field().is_none() {
                    if let Some(missing) = missing {
                        self.b.drop_done(missing);
                    }
                    break;
                }
            }
        }
        self.brackets -= 1;
        self.expect(T::RCurly, "'}' expected");
        Some(self.b.done(m, N::TableExpr))
    }

    fn table_field(&mut self) -> Option<Done> {
        let m = match self.b.tok() {
            Some(T::LBrack) => {
                let m = self.b.mark();
                self.b.advance();
                self.expect_expr();
                self.expect(T::RBrack, "']' expected");
                // CfxLua set constructor: `{ [k] }` stores true.
                if self.b.eat(T::Assign) {
                    self.expect_expr();
                }
                m
            }
            Some(T::Dot) => {
                let m = self.b.mark();
                self.b.advance();
                self.expect(T::Id, "ID expected");
                if self.b.eat(T::Assign) {
                    self.expect_expr();
                }
                m
            }
            Some(T::Id) if self.b.look_ahead(1) == Some(T::Assign) => {
                let m = self.b.mark();
                self.b.advance();
                self.b.advance();
                self.expect_expr();
                m
            }
            _ => {
                let expr = self.expr()?;
                self.b.precede(expr)
            }
        };
        Some(self.finish(m, N::TableField))
    }
}
