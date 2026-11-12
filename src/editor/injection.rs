//! `---@language JSON` above a statement: the strings of that statement are text of that language
//! (`LuaLanguageInjector`).

use super::selection::string_content;
use crate::lua::Source;
use crate::lua::doc::tags::Comment;
use crate::lua::tree::N;
use crate::ty::psi;

const TAG: &str = "@language";

/// The most ranges OpenIDEA routes for one document.
const MAX: usize = 1000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Injected {
    pub start: usize,
    pub end: usize,
    /// The id of the language in the editor.
    pub language: String,
}

/// The id OpenIDEA gives to the language a tag names: the JetBrains ids are capitalised, and a few are
/// spelled otherwise.
fn language_id(named: &str) -> Option<String> {
    let name = named.trim_matches(['"', '\'']).trim();
    let id = match name.to_ascii_lowercase().as_str() {
        "" => return None,
        "js" | "ecmascript 6" => "javascript".to_owned(),
        "ts" => "typescript".to_owned(),
        "regexp" => "regex".to_owned(),
        "shell script" | "bash" | "sh" => "shellscript".to_owned(),
        "text" => "plaintext".to_owned(),
        other => other.to_owned(),
    };
    Some(id)
}

/// What the strings of the statements under a `@language` comment hold, in the order of the text.
pub fn injections(src: &Source) -> Vec<Injected> {
    if !src.text.contains(TAG) {
        return Vec::new();
    }
    let mut found = Vec::new();
    for comment in src.docs().children_of(N::DocComment) {
        let Some(tag) = comment.child(N::DocTagLan) else { continue };
        let Some(language) = tag.child(N::DocLanId).and_then(|id| language_id(id.code())) else { continue };
        let Some(owner) = src.documented_by(Comment(comment)) else { continue };
        // A string is of the statement, or of the table field, that most closely holds it.
        let holder = |node| {
            std::iter::successors(Some(node), |inner: &crate::lua::tree::Node| inner.parent())
                .find(|outer| outer.kind().is_statement() || outer.is(N::TableField))
        };
        for literal in owner.descendants().filter(|node| psi::is_string(*node) && holder(*node) == Some(owner)) {
            if let Some((start, end)) = string_content(&src.text, literal.code_range()).filter(|(from, to)| from < to) {
                found.push(Injected { start, end, language: language.clone() });
            }
        }
    }
    found.sort_by_key(|injected| injected.start);
    found.truncate(MAX);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each injected range as `language: text`.
    fn injected(text: &str) -> Vec<String> {
        let src = Source::new(text);
        injections(&src).iter().map(|found| format!("{}: {}", found.language, &text[found.start..found.end])).collect()
    }

    #[test]
    fn the_strings_of_a_statement_are_of_the_language_its_comment_names() {
        assert_eq!(injected("---@language JSON\nlocal data = '{\"a\": 1}'"), ["json: {\"a\": 1}"]);
        assert_eq!(
            injected("---@language SQL\nMySQL.query([[\n  SELECT * FROM users\n]], { id })"),
            ["sql: \n  SELECT * FROM users\n"]
        );
        assert_eq!(injected("---@language \"HTML\"\nSetPage('<b>x</b>', 'y')"), ["html: <b>x</b>", "html: y"]);
        assert_eq!(injected("---@language js\nlocal a, b = 'f()', \"g()\""), ["javascript: f()", "javascript: g()"]);
    }

    #[test]
    fn a_string_of_another_statement_is_left_alone() {
        let text = "---@language JSON\nlocal function load()\n    local inner = '{}'\n    return '[]'\nend\nlocal after = '{}'";
        assert!(injected(text).is_empty(), "the strings of the body are of their own statements");
        assert!(injected("---@language\nlocal a = '{}'").is_empty(), "no language is named");
        assert!(injected("-- @language JSON is not a tag\nlocal a = '{}'").is_empty());
        assert!(injected("---@language JSON\nlocal a = ''").is_empty(), "nothing to colour");
        // A field of a table is documented by its own comment, not by the one of the statement around it.
        assert!(injected("---@language HTML\nSendNUIMessage({ html = '<b>x</b>' })").is_empty());
        let table = "local t = {\n    ---@language JSON\n    body = '{}',\n    other = '{}',\n}";
        assert_eq!(injected(table), ["json: {}"]);
    }
}
