//! The white space between two blocks: what each block class asks for (`getSpacing`), then the rules of
//! `LuaFormattingModelBuilder.createSpaceBuilder`, the first one that matches.

use super::Style;
use super::blocks::{Blocks, Kind, NONE, Ty};
use crate::lua::token::T;
use crate::lua::tree::{N, Tree};

/// What IntelliJ's `Spacing.createSpacing` takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Spacing {
    pub min: u32,
    pub max: u32,
    pub min_lf: u32,
    pub keep_lf: bool,
    pub keep_blank: u32,
}

const fn spacing(min: u32, max: u32, min_lf: u32, keep_lf: bool, keep_blank: u32) -> Spacing {
    Spacing { min, max, min_lf, keep_lf, keep_blank }
}

/// One space, on the line or kept on the next one: what `and`, `or`, `not` and a simple block take.
const ONE: Spacing = spacing(1, 1, 0, true, 1);
/// Two statements are on two lines, one blank line kept between them.
const STATEMENTS: Spacing = spacing(1, 0, 1, true, 1);

pub(super) struct Rules<'a> {
    pub tree: &'a Tree,
    pub src: &'a str,
    pub blocks: &'a Blocks,
    pub style: &'a Style,
}

/// The tokens `LuaParserDefinition.KEYWORD_TOKENS` holds: the words that are no value, `::` and `goto`.
fn is_keyword(ty: Ty) -> bool {
    matches!(
        ty,
        Ty::T(
            T::And
                | T::Break
                | T::Do
                | T::Else
                | T::ElseIf
                | T::End
                | T::For
                | T::Function
                | T::If
                | T::In
                | T::Local
                | T::Not
                | T::Or
                | T::Repeat
                | T::Return
                | T::Then
                | T::Until
                | T::While
                | T::DoubleColon
                | T::Goto
        )
    )
}

/// The tokens around which a block is simple (`LuaIndentBlock`).
fn bounds_simple_block(ty: Ty) -> bool {
    matches!(ty, Ty::T(T::Do | T::End | T::If | T::Else | T::ElseIf | T::While | T::Then | T::RParen))
}

impl Rules<'_> {
    fn has_token(&self, block: u32, wanted: impl Fn(T) -> bool) -> bool {
        let node = self.blocks.node(self.tree, self.src, block);
        node.is_some_and(|node| node.tokens().any(|token| wanted(token.kind())))
    }

    /// Whether at most one block stands between `block` and the next bound that way (`isSimpleBlock`).
    fn is_simple(&self, block: u32, next: impl Fn(u32) -> u32) -> bool {
        let near = next(block);
        if near == NONE || bounds_simple_block(self.blocks.blocks[near as usize].ty) {
            return true;
        }
        let far = next(near);
        far != NONE && bounds_simple_block(self.blocks.blocks[far as usize].ty)
    }

    fn rule(&self, parent: Ty, a: Ty, b: Ty) -> Option<Spacing> {
        let style = self.style;
        let spaces = |count: bool| Some(spacing(count as u32, count as u32, 0, true, style.keep_blank_lines));
        let line_break = Some(spacing(1, 0, 1, true, style.keep_blank_lines));
        let either = |kind: T| a == Ty::T(kind) || b == Ty::T(kind);
        let node = |kind: N| Ty::N(kind);
        let compound = |ty: Ty| matches!(ty, Ty::T(kind) if kind.is_compound_assign());
        if b == Ty::T(T::End) || a == Ty::T(T::Do) || a == Ty::T(T::Then) || either(T::Else) || b == Ty::T(T::ElseIf) {
            return line_break;
        }
        if a == Ty::T(T::Local) {
            return spaces(true);
        }
        if either(T::Colon) {
            return spaces(false);
        }
        if b == Ty::T(T::Comma) {
            return spaces(style.space_before_comma);
        }
        if a == Ty::T(T::Comma) {
            return spaces(style.space_after_comma);
        }
        if (a == Ty::T(T::LCurly) && b == node(N::TableField)) || (a == node(N::TableField) && b == Ty::T(T::RCurly)) {
            return spaces(style.space_inside_inline_table);
        }
        if b == node(N::TableFieldSep) {
            return spaces(false);
        }
        if a == node(N::TableFieldSep) {
            return spaces(style.space_after_field_sep);
        }
        if parent == node(N::FuncBody) && a == Ty::T(T::RParen) {
            return line_break;
        }
        if a == Ty::T(T::Function) && b == node(N::FuncBody) {
            return spaces(false);
        }
        if a == Ty::T(T::Function) && b == node(N::NameDef) {
            return spaces(true);
        }
        if a == node(N::BinaryOp) || b == node(N::BinaryOp) {
            return spaces(style.space_around_binary_operator);
        }
        if a == node(N::UnaryOp) || b == node(N::UnaryOp) {
            return spaces(false);
        }
        // No line break on either side of `=` and of the CfxLua compound operators.
        if either(T::Assign) || compound(a) || compound(b) {
            let count = style.space_around_assignment as u32;
            return Some(spacing(count, count, 0, false, 0));
        }
        if either(T::Question) {
            return spaces(false);
        }
        // `::` is one of the keyword tokens of the JetBrains plugin, which writes `:: name ::`: a label keeps
        // the spaces it is written with.
        if parent == node(N::LabelStat) {
            return None;
        }
        if is_keyword(a) || is_keyword(b) {
            return spaces(true);
        }
        if either(T::LBrack) || b == Ty::T(T::RBrack) || a == Ty::T(T::LParen) || b == Ty::T(T::RParen) {
            return spaces(false);
        }
        if b == Ty::T(T::Semi) {
            return spaces(false);
        }
        None
    }

    /// The spacing between `first` and `second`, two children of `parent` that follow one another.
    pub fn between(&self, parent: u32, first: u32, second: u32) -> Option<Spacing> {
        let blocks = &self.blocks.blocks;
        let (a, b) = (blocks[first as usize].ty, blocks[second as usize].ty);
        match blocks[parent as usize].kind {
            Kind::IndentRange | Kind::FuncBody if self.style.keep_simple_blocks => {
                let before = bounds_simple_block(b) && self.is_simple(second, |block| blocks[block as usize].prev);
                let after = bounds_simple_block(a) && self.is_simple(first, |block| blocks[block as usize].next);
                if before || after {
                    return Some(ONE);
                }
            }
            Kind::Binary => {
                let word = |kind: T| matches!(kind, T::And | T::Not | T::Or);
                if self.has_token(first, word) || self.has_token(second, word) {
                    return Some(ONE);
                }
            }
            Kind::Unary if self.has_token(first, |kind| kind == T::Not) => return Some(ONE),
            // `call(arguments)`, and `call "string"` or `call { table }`.
            Kind::Call if a != Ty::T(T::Question) && b != Ty::T(T::Question) => {
                let glued = self.has_token(second, |kind| kind == T::LParen);
                return Some(spacing(!glued as u32, !glued as u32, 0, false, 0));
            }
            _ => {}
        }
        if self.blocks.is_statement(first) && self.blocks.is_statement(second) {
            return Some(STATEMENTS);
        }
        self.rule(blocks[parent as usize].ty, a, b)
    }
}

/// The spacing before each leaf: the one its topmost block that starts there takes from its parent, after
/// the block before it.
pub(super) fn of_leaves(rules: &Rules) -> Vec<Option<Spacing>> {
    let blocks = &rules.blocks.blocks;
    let mut out = vec![None; rules.blocks.leaves.len()];
    for (id, block) in blocks.iter().enumerate() {
        if block.prev != NONE {
            out[block.first as usize] = rules.between(block.parent, block.prev, id as u32);
        }
    }
    out
}
