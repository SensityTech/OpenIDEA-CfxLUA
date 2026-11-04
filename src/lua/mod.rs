//! Lua as the plugin sees it: tokens and call structure, nothing more. The
//! IDE's own Lua support (tree-sitter, lua-language-server) does the rest.

pub mod joaat;
pub mod lexer;
pub mod syntax;

pub use syntax::{Call, Chain, Seg, Source};
