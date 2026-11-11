//! Documentation comments (`---@param name T`): the lexer and the grammar of the JetBrains plugin, which
//! reads EmmyDoc and the LuaCATS annotations of lua-language-server.

mod lexer;
mod parser;
pub mod tags;

#[cfg(test)]
mod tests;
