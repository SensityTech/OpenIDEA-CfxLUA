use super::lexer;
use crate::lua::builder::{Builder, Done};
use crate::lua::token::{T, Tokens};
use crate::lua::tree::{N, Tree};
use std::rc::Rc;

const MAX_DEPTH: usize = 100;

impl Tree {
    /// The documentation comments of `src`, whose tokens are `tokens`: a `DocFile` that holds one
    /// `DocComment` per comment. Its own tokens cover `src` end to end, the code between two comments
    /// being white space.
    pub fn docs(src: &str, tokens: &Tokens) -> Tree {
        let mut docs = Tokens::with_capacity(0);
        let mut comments: Vec<(usize, usize)> = Vec::new();
        let mut covered = 0;
        for i in (0..tokens.len()).filter(|i| tokens.kind(*i) == T::DocComment) {
            let (start, end) = (tokens.start(i), tokens.end(i));
            if start > covered {
                docs.push(T::Whitespace, covered);
            }
            let first = docs.len();
            lexer::lex(src, start, end, &mut docs);
            comments.push((first, docs.len()));
            covered = end;
        }
        if covered < src.len() || docs.is_empty() {
            docs.push(T::Whitespace, covered);
        }
        docs.close(src.len());
        let docs = Rc::new(docs);
        let (nodes, errors) = {
            let mut parser = Parser { b: Builder::new(src, &docs), depth: 0 };
            let file = parser.b.mark();
            for (first, end) in comments {
                parser.b.enter(first, end);
                parser.comment();
            }
            parser.b.enter(docs.len(), docs.len());
            parser.b.done(file, N::DocFile);
            parser.b.finish()
        };
        Tree { tokens: docs, nodes, errors }
    }
}

/// The grammar of the JetBrains plugin (`doc.bnf`). A rule that read its first token stays, whatever is
/// missing after it; a line keeps nothing after what its tag reads.
struct Parser<'a> {
    b: Builder<'a>,
    depth: usize,
}

impl Parser<'_> {
    fn expect(&mut self, kind: T, message: &'static str) {
        if !self.b.eat(kind) {
            self.b.error(message);
        }
    }

    /// The next token is `first` and the one after it `second`.
    fn at_pair(&mut self, first: T, second: T) -> bool {
        self.b.at(first) && self.b.look_ahead(1) == Some(second)
    }

    /// Reads `sign`, then what `rest` reads; neither when `rest` reads nothing.
    fn after(&mut self, sign: T, rest: fn(&mut Self) -> bool) -> bool {
        if !self.b.at(sign) {
            return false;
        }
        let (position, limit) = (self.b.position(), self.b.limit());
        self.b.advance();
        let found = rest(self);
        if !found {
            self.b.enter(position, limit);
        }
        found
    }

    fn leaf(&mut self, kind: N) -> Done {
        let m = self.b.mark();
        self.b.advance();
        self.b.done(m, kind)
    }

    fn comment(&mut self) {
        let m = self.b.mark();
        while !self.b.eof() {
            if self.b.eat(T::DocDashes) {
                match self.b.tok() {
                    Some(T::DocAt) => {
                        self.item();
                    }
                    Some(T::DocString) => self.b.advance(),
                    Some(T::DocAliasBar) => {
                        self.alias_member();
                    }
                    _ => {}
                }
            }
            if !self.b.eof() && !self.b.at(T::DocDashes) {
                let unread = self.b.mark();
                while !self.b.eof() && !self.b.at(T::DocDashes) {
                    self.b.advance();
                }
                self.b.fail(unread, "Unexpected text");
            }
        }
        self.b.done(m, N::DocComment);
    }

    fn item(&mut self) -> bool {
        let m = self.b.mark();
        self.b.advance();
        let found = match self.b.tok() {
            Some(T::DocTagParam) => self.tag_param(),
            Some(T::DocTagAlias) => self.tag_alias(),
            Some(T::DocTagSuppress) => self.tag_suppress(),
            Some(T::DocTagVararg) => self.tag_typed(N::DocTagVararg),
            Some(T::DocTagReturn) => self.tag_return(),
            Some(T::DocTagClass | T::DocTagModule | T::DocTagEnum) => self.tag_class(),
            Some(T::DocTagField) => self.tag_field(),
            Some(T::DocTagType) => self.tag_typed(N::DocTagType),
            Some(T::DocTagLanguage) => self.tag_lan(),
            Some(T::DocTagOverload) => self.tag_overload(),
            Some(T::DocTagSee) => self.tag_see(),
            Some(T::DocTagCast) => self.tag_cast(),
            Some(T::DocTagName) => self.tag_def(),
            Some(T::DocTagPrivate | T::DocTagProtected | T::DocTagPublic) => {
                self.leaf(N::DocAccessModifier);
                true
            }
            Some(T::DocTagGeneric) => self.tag_generic_list(),
            _ => false,
        };
        if found {
            self.b.drop(m);
        } else {
            self.b.rollback(m);
        }
        found
    }

    fn comment_string(&mut self) {
        if matches!(self.b.tok(), Some(T::DocStringBegin | T::DocString)) {
            let m = self.b.mark();
            self.b.eat(T::DocStringBegin);
            self.b.eat(T::DocString);
            self.b.done(m, N::DocCommentString);
        }
    }

    fn alias_member(&mut self) -> bool {
        let m = self.b.mark();
        self.b.advance();
        if !self.ty() {
            self.b.error("Type expected");
        }
        self.comment_string();
        self.b.done(m, N::DocAliasMember);
        true
    }

    /// The `---| value` lines that follow a parameter or an alias.
    fn alias_member_lines(&mut self) {
        while self.at_pair(T::DocDashes, T::DocAliasBar) {
            self.b.advance();
            self.alias_member();
        }
    }

    fn tag_param(&mut self) -> bool {
        let m = self.b.mark();
        self.b.advance();
        if self.b.at(T::Id) {
            self.leaf(N::DocParamNameRef);
        } else {
            self.b.error("ID expected");
        }
        self.b.eat(T::Question);
        self.ty();
        self.comment_string();
        self.alias_member_lines();
        self.b.done(m, N::DocTagParam);
        true
    }

    fn tag_alias(&mut self) -> bool {
        let m = self.b.mark();
        self.b.advance();
        self.expect(T::Id, "ID expected");
        self.ty();
        self.comment_string();
        self.alias_member_lines();
        self.b.done(m, N::DocTagAlias);
        true
    }

    fn tag_suppress(&mut self) -> bool {
        let m = self.b.mark();
        self.b.advance();
        self.expect(T::Id, "ID expected");
        while self.at_pair(T::Comma, T::Id) {
            self.b.advance();
            self.b.advance();
        }
        self.b.done(m, N::DocTagSuppress);
        true
    }

    /// `@vararg` and `@type`: a type, then a description.
    fn tag_typed(&mut self, kind: N) -> bool {
        let m = self.b.mark();
        self.b.advance();
        if !self.ty() {
            self.b.error("Type expected");
        }
        self.comment_string();
        self.b.done(m, kind);
        true
    }

    fn tag_return(&mut self) -> bool {
        let m = self.b.mark();
        self.b.advance();
        if !self.type_list() {
            self.b.error("Type expected");
        }
        self.comment_string();
        self.b.done(m, N::DocTagReturn);
        true
    }

    /// Types, each one possibly followed by the name of the value.
    fn type_list(&mut self) -> bool {
        let m = self.b.mark();
        if !self.ty() {
            self.b.rollback(m);
            return false;
        }
        self.b.eat(T::Id);
        while self.after(T::Comma, Self::ty) {
            self.b.eat(T::Id);
        }
        self.b.done(m, N::DocTypeList);
        true
    }

    fn class_name_ref(&mut self) -> bool {
        let found = self.b.at(T::Id);
        if found {
            self.leaf(N::DocClassNameRef);
        }
        found
    }

    /// `@class`, `@module` and `@enum`: nothing is kept of one that has no name.
    fn tag_class(&mut self) -> bool {
        let m = self.b.mark();
        self.b.advance();
        if self.b.at(T::LParen) {
            let attr = self.b.mark();
            self.b.advance();
            self.expect(T::Id, "ID expected");
            while self.at_pair(T::Comma, T::Id) {
                self.b.advance();
                self.b.advance();
            }
            self.expect(T::RParen, "')' expected");
            self.b.done(attr, N::DocClassAttr);
        }
        if !self.b.eat(T::Id) {
            self.b.rollback(m);
            return false;
        }
        if self.after(T::Colon, Self::class_name_ref) {
            while self.after(T::Comma, Self::class_name_ref) {}
        }
        self.comment_string();
        self.b.done(m, N::DocTagClass);
        true
    }

    fn tag_field(&mut self) -> bool {
        let m = self.b.mark();
        self.b.advance();
        if matches!(
            self.b.tok(),
            Some(
                T::DocPrivate
                    | T::DocProtected
                    | T::DocPublic
                    | T::DocTagPrivate
                    | T::DocTagProtected
                    | T::DocTagPublic
            )
        ) {
            self.leaf(N::DocAccessModifier);
        }
        if self.at_pair(T::Lt, T::Id) && self.b.look_ahead(2) == Some(T::Gt) {
            self.b.advance();
            self.class_name_ref();
            self.b.advance();
        }
        // An index signature `[K] V` has no name.
        let index = self.b.mark();
        if self.b.eat(T::LBrack) && self.ty() && self.b.eat(T::RBrack) {
            self.b.drop(index);
        } else {
            self.b.rollback(index);
            if self.b.eat(T::Id) {
                self.b.eat(T::Question);
            } else {
                self.b.error("ID expected");
            }
        }
        if !self.ty() {
            self.b.error("Type expected");
        }
        self.comment_string();
        self.b.done(m, N::DocTagField);
        true
    }

    fn tag_lan(&mut self) -> bool {
        let m = self.b.mark();
        self.b.advance();
        if matches!(self.b.tok(), Some(T::Id | T::String)) {
            self.leaf(N::DocLanId);
        } else {
            self.b.error("ID expected");
        }
        self.comment_string();
        self.b.done(m, N::DocTagLan);
        true
    }

    fn tag_overload(&mut self) -> bool {
        let m = self.b.mark();
        self.b.advance();
        if self.b.at(T::DocFun) {
            self.function_ty();
        } else {
            self.b.error("'fun' expected");
        }
        self.b.done(m, N::DocTagOverload);
        true
    }

    fn tag_see(&mut self) -> bool {
        let m = self.b.mark();
        self.b.advance();
        if !self.class_name_ref() {
            self.b.error("ID expected");
        }
        if self.at_pair(T::GetN, T::Id) {
            self.b.advance();
            self.b.advance();
        }
        self.comment_string();
        self.b.done(m, N::DocTagSee);
        true
    }

    /// `+T`, `-T`, `T`, `+?` or `-?`.
    fn cast_item(&mut self) -> bool {
        let m = self.b.mark();
        if !self.b.eat(T::Plus) {
            self.b.eat(T::Minus);
        }
        let found = self.ty() || self.b.eat(T::Question);
        if found {
            self.b.done(m, N::DocCastItem);
        } else {
            self.b.rollback(m);
        }
        found
    }

    fn tag_cast(&mut self) -> bool {
        let m = self.b.mark();
        self.b.advance();
        self.expect(T::Id, "ID expected");
        if self.cast_item() {
            while self.after(T::Comma, Self::cast_item) {}
        } else {
            self.b.error("Type expected");
        }
        self.comment_string();
        self.b.done(m, N::DocTagCast);
        true
    }

    fn tag_def(&mut self) -> bool {
        let m = self.b.mark();
        self.b.advance();
        self.comment_string();
        self.b.done(m, N::DocTagDef);
        true
    }

    fn generic_def(&mut self) -> bool {
        if !self.b.at(T::Id) {
            return false;
        }
        let m = self.b.mark();
        self.b.advance();
        self.after(T::Colon, Self::class_name_ref);
        self.b.done(m, N::DocGenericDef);
        true
    }

    /// Names separated by commas, each one possibly bounded by a class.
    fn generic_defs(&mut self) {
        if self.generic_def() {
            while self.after(T::Comma, Self::generic_def) {}
        } else {
            self.b.error("ID expected");
        }
    }

    fn tag_generic_list(&mut self) -> bool {
        let m = self.b.mark();
        self.b.advance();
        self.generic_defs();
        self.b.done(m, N::DocTagGenericList);
        true
    }

    fn ty(&mut self) -> bool {
        self.ty_above(-1)
    }

    fn operand(&mut self) -> bool {
        self.ty_above(0)
    }

    /// A type whose operators bind tighter than `priority`: a union is 0, the arguments of a generic 3,
    /// `[]` 4 and `?` 5.
    fn ty_above(&mut self, priority: i8) -> bool {
        if self.depth >= MAX_DEPTH {
            self.b.error("Type is nested too deeply");
            return false;
        }
        self.depth += 1;
        let found = self.operators(priority);
        self.depth -= 1;
        found
    }

    fn operators(&mut self, priority: i8) -> bool {
        let Some((mut left, mut kind)) = self.atom() else { return false };
        loop {
            (left, kind) = match self.b.tok() {
                Some(T::BitOr) if priority < 0 => {
                    let m = self.b.precede(left);
                    while self.b.eat(T::BitOr) {
                        if !self.operand() {
                            self.b.error("Type expected");
                        }
                    }
                    (self.b.done(m, N::DocUnionTy), N::DocUnionTy)
                }
                Some(T::Lt) if priority < 3 && kind == N::DocGeneralTy => {
                    let m = self.b.precede(left);
                    self.b.advance();
                    if !self.ty() {
                        self.b.error("Type expected");
                    }
                    while self.b.eat(T::Comma) {
                        if !self.ty() {
                            self.b.error("Type expected");
                        }
                    }
                    self.expect(T::Gt, "'>' expected");
                    (self.b.done(m, N::DocGenericTy), N::DocGenericTy)
                }
                Some(T::DocArr) if priority < 4 => {
                    let m = self.b.precede(left);
                    self.b.advance();
                    (self.b.done(m, N::DocArrTy), N::DocArrTy)
                }
                Some(T::Question) if priority < 5 => {
                    let m = self.b.precede(left);
                    self.b.advance();
                    (self.b.done(m, N::DocNullableTy), N::DocNullableTy)
                }
                _ => return true,
            };
        }
    }

    fn atom(&mut self) -> Option<(Done, N)> {
        let kind = match self.b.tok()? {
            T::DocFun => return Some((self.function_ty(), N::DocFunctionTy)),
            T::LCurly => return Some((self.table_ty(), N::DocTableTy)),
            T::LBrack => return Some((self.tuple_ty(), N::DocTupleTy)),
            T::LParen => return Some((self.par_ty(), N::DocParTy)),
            T::Id => {
                let m = self.b.mark();
                self.leaf(N::DocClassNameRef);
                return Some((self.b.done(m, N::DocGeneralTy), N::DocGeneralTy));
            }
            T::String => N::DocStringLiteralTy,
            T::Number => N::DocNumberLiteralTy,
            _ => return None,
        };
        Some((self.leaf(kind), kind))
    }

    fn par_ty(&mut self) -> Done {
        let m = self.b.mark();
        self.b.advance();
        if !self.ty() {
            self.b.error("Type expected");
        }
        self.expect(T::RParen, "')' expected");
        self.b.done(m, N::DocParTy)
    }

    fn tuple_ty(&mut self) -> Done {
        let m = self.b.mark();
        self.b.advance();
        if self.ty() {
            while self.after(T::Comma, Self::ty) {}
        } else {
            self.b.error("Type expected");
        }
        self.expect(T::RBrack, "']' expected");
        self.b.done(m, N::DocTupleTy)
    }

    /// `name: T`, `name?: T` or `[K]: V`.
    fn table_field(&mut self) -> bool {
        let kind = match self.b.tok() {
            Some(T::Id) => N::DocTableField,
            Some(T::LBrack) => N::DocTableIndexField,
            _ => return false,
        };
        let m = self.b.mark();
        self.b.advance();
        if kind == N::DocTableField {
            self.b.eat(T::Question);
        } else {
            if !self.ty() {
                self.b.error("Type expected");
            }
            self.expect(T::RBrack, "']' expected");
        }
        self.expect(T::Colon, "':' expected");
        if !self.ty() {
            self.b.error("Type expected");
        }
        self.b.done(m, kind);
        true
    }

    fn table_ty(&mut self) -> Done {
        let ty = self.b.mark();
        let def = self.b.mark();
        self.b.advance();
        if self.table_field() {
            while self.b.eat(T::Comma) && self.table_field() {}
        }
        self.expect(T::RCurly, "'}' expected");
        self.b.done(def, N::DocTableDef);
        self.b.done(ty, N::DocTableTy)
    }

    fn function_param(&mut self) {
        let m = self.b.mark();
        self.b.advance();
        self.b.eat(T::Question);
        self.after(T::Colon, Self::ty);
        self.b.done(m, N::DocFunctionParam);
    }

    /// Parameters separated by commas; the last one may be `vararg T`, and a comma may end the list.
    fn function_params(&mut self) {
        loop {
            match self.b.tok() {
                Some(T::Id) => {
                    self.function_param();
                    if !self.b.eat(T::Comma) {
                        return;
                    }
                }
                Some(T::DocVararg) => {
                    let m = self.b.mark();
                    self.b.advance();
                    if !self.ty() {
                        self.b.error("Type expected");
                    }
                    self.b.done(m, N::DocVarargParam);
                    return;
                }
                Some(T::RParen) => return,
                _ => {
                    self.b.error("Parameter expected");
                    return;
                }
            }
        }
    }

    fn function_ty(&mut self) -> Done {
        let m = self.b.mark();
        self.b.advance();
        if self.b.eat(T::Lt) {
            self.generic_defs();
            self.expect(T::Gt, "'>' expected");
        }
        self.expect(T::LParen, "'(' expected");
        self.function_params();
        self.expect(T::RParen, "')' expected");
        self.after(T::Colon, Self::type_list);
        self.b.done(m, N::DocFunctionTy)
    }
}
