//! What the IntelliJ platform does with blocks and spacings (`com.intellij.formatting`): each white space
//! is arranged in the order of the text, then the token after it is indented or aligned. The indent of a
//! token that starts a line is the sum of the indents of the blocks that start with it, added to the indent
//! of the line where the first block around them starts (`AbstractBlockWrapper.getChildOffset`).

use super::Style;
use super::blocks::{Blocks, Indent, NONE};
use super::spacing::Spacing;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};

/// What a layout may do before it gives a file up: `WORK` for each token, and `SPARE` whatever the size.
/// A file written by hand asks for about ten a token; one whose `=` are aligned, where each longer name
/// sends the layout back, for over a hundred.
const WORK: usize = 64;
const SPARE: usize = 1 << 23;

/// The white space before a token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Gap {
    pub lf: u32,
    /// The spaces before the token, or the indent of its line where `lf` is not 0.
    pub cols: u32,
    /// Outside of what is formatted: kept as it is.
    pub read_only: bool,
}

/// What the layout reads of a token.
#[derive(Clone, Copy, Debug)]
pub(super) struct Shape {
    /// Its width, or the width of its last line where it spans several.
    pub width: u32,
    pub multiline: bool,
    /// A comment that runs to the end of its line: the line break after it stays.
    pub ends_line: bool,
    /// Whether the token touched the one before it in the source.
    pub glued: bool,
    /// Whether written without a space after the token before it, the two would read as other tokens.
    pub merges: bool,
}

pub(super) struct Layout<'a> {
    pub blocks: &'a mut Blocks,
    pub spacings: &'a [Option<Spacing>],
    pub shapes: &'a [Shape],
    pub style: &'a Style,
    pub gaps: Vec<Gap>,
    /// The white spaces as they were read: what an alignment that is given up goes back to.
    initial: Vec<Gap>,
    /// The last token arranged so far.
    reached: usize,
    /// The column each token starts at, and the one it ends at on its last line.
    col: Vec<u32>,
    end_col: Vec<u32>,
    /// The line each token starts on.
    line: Vec<u32>,
    skipped: HashSet<u32>,
    /// The blocks a block was aligned with, both ways.
    aligned: HashMap<u32, Vec<u32>>,
    /// The block last moved to the right, and the block that moved it.
    shifted: Option<(u32, u32)>,
    /// What the layout may still do: a block passed on the way to an indent or to an alignment, a token read
    /// again. A long sum is as deep as it is long, and a block that moves sends the layout back.
    work: Cell<usize>,
}

impl<'a> Layout<'a> {
    pub fn new(
        blocks: &'a mut Blocks,
        spacings: &'a [Option<Spacing>],
        shapes: &'a [Shape],
        style: &'a Style,
        gaps: Vec<Gap>,
    ) -> Self {
        let count = gaps.len();
        Self {
            blocks,
            spacings,
            shapes,
            style,
            initial: gaps.clone(),
            gaps,
            reached: 0,
            col: vec![0; count],
            end_col: vec![0; count],
            line: vec![0; count],
            skipped: HashSet::new(),
            aligned: HashMap::new(),
            shifted: None,
            work: Cell::new(count * WORK + SPARE),
        }
    }

    fn spend(&self, work: usize) {
        self.work.set(self.work.get().saturating_sub(work));
    }

    /// Whether a token is the first one of its line, the first token of the file included
    /// (`WhiteSpace.containsLineFeeds`).
    fn starts_line(&self, leaf: u32) -> bool {
        leaf == 0 || self.gaps[leaf as usize].lf > 0
    }

    /// `WhiteSpace.arrangeLineFeeds` and `arrangeSpaces`, then what keeps the tokens what they are: a
    /// comment that ends its line keeps its line break, two tokens that would merge keep a space.
    fn arrange(&mut self, leaf: usize) {
        let mut gap = self.gaps[leaf];
        if gap.read_only {
            return;
        }
        match self.spacings[leaf] {
            Some(rule) => {
                gap.lf = gap.lf.max(rule.min_lf);
                if gap.lf > 0 {
                    if rule.keep_blank > 0 {
                        gap.lf = gap.lf.min(rule.keep_blank + 1);
                    } else if gap.lf > rule.min_lf {
                        gap.lf = if rule.keep_lf { rule.min_lf.max(1) } else { rule.min_lf };
                    }
                    if gap.lf == 1 && rule.min_lf == 0 && !rule.keep_lf {
                        gap.lf = 0;
                    }
                    if gap.lf == 0 {
                        gap.cols = 0;
                    }
                }
                if gap.lf == 0 && leaf > 0 {
                    if gap.cols < rule.min {
                        gap.cols = rule.min;
                    }
                    if gap.cols > rule.max {
                        gap.cols = rule.max;
                    }
                }
            }
            None if leaf == 0 => gap = Gap { lf: 0, cols: 0, read_only: false },
            None => {}
        }
        if leaf > 0 && gap.lf == 0 {
            let shape = self.shapes[leaf];
            if self.shapes[leaf - 1].ends_line {
                gap.lf = 1;
            } else if gap.cols == 0 && !shape.glued && shape.merges {
                gap.cols = 1;
            }
        }
        self.gaps[leaf] = gap;
    }

    /// The size of the indent of a block, for the token `target` that starts a line
    /// (`CoreFormatterUtil.getIndent`).
    fn indent_of(&self, block: u32, target: u32) -> u32 {
        let block = &self.blocks.blocks[block as usize];
        match block.indent {
            Indent::None => 0,
            Indent::Normal => self.style.indent,
            Indent::Continuation => self.style.continuation,
            Indent::ContinuationWithoutFirst => {
                let first = block.parent != NONE && self.blocks.blocks[block.parent as usize].first == block.first;
                if !first && block.first == target { self.style.continuation } else { 0 }
            }
        }
    }

    /// The block the blocks of an alignment line up with, for one that starts at `leaf`: the first one
    /// aligned, unless the last one before `leaf` is on its line (`AlignmentImpl.getOffsetRespBlockBefore`).
    fn anchor(&self, align: u32, leaf: u32) -> Option<u32> {
        let align = &self.blocks.aligns[align as usize];
        if !align.backward && !self.starts_line(leaf) {
            return None;
        }
        let before = align.registered.partition_point(|registered| *registered < leaf);
        let neighbour = *align.registered[..before].last()?;
        (self.line[neighbour as usize] != self.line[leaf as usize]).then(|| align.registered[0])
    }

    /// The alignment a token takes: the one of a block that starts with it, the innermost first, that has a
    /// block to line up with (`CoreFormatterUtil.getAlignment`).
    fn alignment_of(&self, leaf: u32) -> Option<(u32, u32)> {
        let mut block = self.blocks.leaves[leaf as usize].1;
        while block != NONE && self.blocks.blocks[block as usize].first == leaf {
            self.spend(1);
            let align = self.blocks.blocks[block as usize].align;
            if align != NONE
                && let Some(anchor) = self.anchor(align, leaf)
            {
                return Some((align, anchor));
            }
            block = self.blocks.blocks[block as usize].parent;
        }
        None
    }

    /// The indent of a child that starts a line inside a block that is aligned: counted from what that
    /// block lines up with (`AbstractBlockWrapper.createAlignmentIndent`).
    fn aligned_indent(&self, parent: u32, child: u32, child_indent: u32) -> Option<u32> {
        let child = &self.blocks.blocks[child as usize];
        if !self.starts_line(child.first) {
            return None;
        }
        let align = self.blocks.align_at_start(parent);
        if align == NONE || align == child.align {
            return None;
        }
        let start = self.blocks.blocks[parent as usize].first;
        let anchor = self.anchor(align, child.first);
        // No line starts between what the block lines up with and the block: they are on one line.
        if let Some(anchor) = anchor
            && anchor != start
            && self.line[anchor as usize] == self.line[start as usize - 1]
        {
            return None;
        }
        if child.first == 0 {
            return Some(child_indent);
        }
        Some(self.col[anchor.unwrap_or(start) as usize] + child_indent)
    }

    /// The indent of the token `target`, which starts a line; `from` is the block the walk up starts at:
    /// the block of the token, or for a line inside a token that block again, the token being what is around
    /// the line.
    fn offset(&self, target: u32, from: u32, inner: bool) -> u32 {
        let blocks = &self.blocks.blocks;
        let mut sum = 0;
        let mut child = from;
        let mut parent = if inner { from } else { blocks[from as usize].parent };
        let mut virtual_child = inner;
        loop {
            if parent == NONE {
                return sum;
            }
            self.spend(1);
            let around = &blocks[parent as usize];
            let child_indent = match virtual_child {
                true => 0,
                false => {
                    let inside = &blocks[child as usize];
                    let used = !self.starts_line(around.first)
                        && inside.indent != Indent::None
                        && around.line_child < inside.first;
                    if self.starts_line(inside.first) || used { self.indent_of(child, target) } else { 0 }
                }
            };
            sum += child_indent;
            if !virtual_child && around.first == target {
                child = parent;
                parent = around.parent;
                continue;
            }
            let aligned = match virtual_child {
                true => None,
                false => self.aligned_indent(parent, child, child_indent),
            };
            if let Some(aligned) = aligned {
                return sum - child_indent + aligned;
            }
            if self.starts_line(around.first) {
                return sum + self.gaps[around.first as usize].cols;
            }
            virtual_child = false;
            child = parent;
            parent = around.parent;
        }
    }

    /// The indent of the lines a token spans after its first one: a documentation comment has each of its
    /// lines where the first one starts.
    pub fn inner_indent(&self, leaf: u32) -> u32 {
        self.offset(u32::MAX, self.blocks.leaves[leaf as usize].1, true)
    }

    fn remember(&mut self, a: u32, b: u32) {
        self.aligned.entry(a).or_default().push(b);
        self.aligned.entry(b).or_default().push(a);
    }

    /// Whether the block `anchor` may move to the right for `leaf`: not where a block of its line is what a
    /// block before `leaf` on its line is aligned with (`CoreFormatterUtil.allowBackwardAlignment`).
    fn may_shift(&self, anchor: u32, leaf: u32) -> bool {
        let mut held: HashSet<u32> = HashSet::new();
        let mut before = leaf;
        while before > 0 {
            before -= 1;
            self.spend(1);
            held.extend(self.aligned.get(&before).into_iter().flatten());
            if self.starts_line(before) {
                break;
            }
        }
        let mut after = anchor + 1;
        while (after as usize) < self.gaps.len() && !self.starts_line(after) {
            self.spend(1);
            if held.contains(&after) {
                return false;
            }
            after += 1;
        }
        true
    }

    /// Gives an alignment up whose first block was moved without the block that asked for it getting to its
    /// column. IntelliJ leaves the first block where it was moved to, and moves it again at each pass: here
    /// every white space from that block on goes back to what was read, to be arranged without the alignment.
    fn give_up(&mut self, align: u32, anchor: u32) {
        self.skipped.insert(align);
        self.shifted = None;
        let from = anchor as usize;
        self.gaps[from..=self.reached].copy_from_slice(&self.initial[from..=self.reached]);
        self.spend(self.aligned.len() + (self.reached - from) / 16);
        self.aligned.retain(|block, _| *block < anchor);
        for with in self.aligned.values_mut() {
            with.retain(|block| *block < anchor);
        }
    }

    /// Lines the token `leaf` up with `anchor`. Returns the token the layout goes back to where an earlier
    /// block had to move (`AbstractBlockAlignmentProcessor.applyAlignment`, `AlignmentHelper`).
    fn align(&mut self, align: u32, anchor: u32, leaf: u32) -> Option<u32> {
        let wanted = self.col[anchor as usize];
        let at = leaf as usize;
        if self.starts_line(leaf) {
            if !self.gaps[at].read_only {
                self.gaps[at].cols = wanted;
            }
            self.remember(leaf, anchor);
            return None;
        }
        let current = self.end_col[at - 1] + self.gaps[at].cols;
        if wanted >= current {
            if !self.gaps[at].read_only {
                self.gaps[at].cols += wanted - current;
            }
            self.remember(leaf, anchor);
            return None;
        }
        if !self.blocks.aligns[align as usize].backward {
            return None;
        }
        if self.gaps[anchor as usize].read_only || !self.may_shift(anchor, leaf) {
            self.skipped.insert(align);
            return None;
        }
        if self.shifted == Some((anchor, leaf)) {
            self.give_up(align, anchor);
            return Some(anchor);
        }
        let by = current - wanted;
        self.gaps[anchor as usize].cols += by;
        self.col[anchor as usize] += by;
        if !self.shapes[anchor as usize].multiline {
            self.end_col[anchor as usize] += by;
        }
        self.shifted = Some((anchor, leaf));
        self.remember(leaf, anchor);
        Some(anchor + 1)
    }

    /// Records where the token `leaf` ended up: its column, its line, and for each block that starts with
    /// it the alignment it now belongs to and the parent it starts a line in.
    fn settle(&mut self, leaf: u32) {
        let at = leaf as usize;
        let starts = self.starts_line(leaf);
        self.line[at] = if leaf == 0 { 0 } else { self.line[at - 1] + starts as u32 };
        self.col[at] = if starts { self.gaps[at].cols } else { self.end_col[at - 1] + self.gaps[at].cols };
        let shape = self.shapes[at];
        self.end_col[at] = if shape.multiline { shape.width } else { self.col[at] + shape.width };
        let mut block = self.blocks.leaves[at].1;
        while block != NONE && self.blocks.blocks[block as usize].first == leaf {
            self.spend(1);
            let (align, parent) = (self.blocks.blocks[block as usize].align, self.blocks.blocks[block as usize].parent);
            if align != NONE {
                let registered = &mut self.blocks.aligns[align as usize].registered;
                if registered.last().is_none_or(|last| *last < leaf) {
                    registered.push(leaf);
                }
            }
            if starts && parent != NONE {
                let around = &mut self.blocks.blocks[parent as usize];
                around.line_child = around.line_child.min(leaf);
            }
            block = parent;
        }
    }

    /// Arranges every white space (`AdjustWhiteSpacesState.processToken`). Tells whether it got to the end
    /// of the file with the work it may do.
    pub fn run(&mut self) -> bool {
        let count = self.gaps.len();
        let mut leaf = 0;
        while leaf < count && self.work.get() > 0 {
            self.spend(4);
            self.reached = self.reached.max(leaf);
            self.arrange(leaf);
            let index = leaf as u32;
            self.line[leaf] = if leaf == 0 { 0 } else { self.line[leaf - 1] + self.starts_line(index) as u32 };
            match self.alignment_of(index).filter(|(align, _)| !self.skipped.contains(align)) {
                Some((align, anchor)) => {
                    if let Some(back) = self.align(align, anchor, index) {
                        leaf = back as usize;
                        continue;
                    }
                }
                None if self.starts_line(index) && !self.gaps[leaf].read_only => {
                    self.gaps[leaf].cols = self.offset(index, self.blocks.leaves[leaf].1, false);
                }
                None => {}
            }
            self.settle(index);
            leaf += 1;
        }
        leaf == count
    }
}
