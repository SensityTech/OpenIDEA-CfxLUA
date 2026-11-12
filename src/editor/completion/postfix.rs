//! The postfix templates of the JetBrains plugin (`codeInsight/postfix`): after `expr.`, a key that rewrites
//! the expression, as `count.if` gives `if count then ... end`.

use super::Listing;
use crate::lua::Source;
use crate::lua::tree::{N, Node};
use crate::ty::psi;
use openidea_plugin::providers::{CompletionItem, CompletionKind};

/// After the words of the file: a template is asked for by its key.
pub(super) const SORT: &str = "9~";

/// What a template is made from: its key, what it says it writes, and the snippet, where `%` stands for the
/// expression.
struct Template {
    key: &'static str,
    shown: &'static str,
    body: &'static str,
}

const fn template(key: &'static str, shown: &'static str, body: &'static str) -> Template {
    Template { key, shown, body }
}

/// The templates that turn an expression into a statement: offered where the expression is one, alone on
/// its line.
const STATEMENTS: [Template; 12] = [
    template("if", "if expr then end", "if % then\n\t$0\nend"),
    template("if_not", "if not expr then end", "if not % then\n\t$0\nend"),
    template("if_nil", "if expr == nil then end", "if % == nil then\n\t$0\nend"),
    template("if_not_nil", "if expr ~= nil then end", "if % ~= nil then\n\t$0\nend"),
    template("for", "for i = 1, expr do end", "for i = 1, % do\n\t$0\nend"),
    template("for_i", "for i, v in ipairs(expr) do end", "for i, v in ipairs(%) do\n\t$0\nend"),
    template("for_p", "for k, v in pairs(expr) do end", "for k, v in pairs(%) do\n\t$0\nend"),
    template("return", "return expr", "return %"),
    template("print", "print(expr)", "print(%)"),
    template("increase", "expr = expr + value", "% = % + ${1:1}"),
    template("decrease", "expr = expr - value", "% = % - ${1:1}"),
    template("var", "local inst = expr", "local ${1:inst} = %$0"),
];

/// The templates that wrap an expression where it stands.
const EXPRESSIONS: [Template; 3] = [
    template("par", "(expr)", "(%)$0"),
    template("tostring", "tostring(expr)", "tostring(%)"),
    template("tonumber", "tonumber(expr)", "tonumber(%)"),
];

/// A text as a snippet writes it as it is.
fn literal(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '$' | '}' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Whether the expression that starts at `start` is a statement of its own, with nothing after the caret
/// on its line: what follows on the next line is another statement, whatever the parser made of it.
fn alone(src: &Source, index: Node, start: usize, at: usize) -> bool {
    let statement = index.ancestors().find(|outer| outer.kind().is_statement());
    let first = statement.is_some_and(|statement| statement.code_range().0 == start);
    let rest = &src.text[at..];
    first && rest[..rest.find('\n').unwrap_or(rest.len())].trim().is_empty()
}

/// The templates offered after the `.` that is the code token `sign`, the caret being at `at`.
pub(super) fn templates(listing: &mut Listing, src: &Source, sign: usize, at: usize) {
    let sign = src.toks[sign];
    if sign.text(&src.text) != "." {
        return;
    }
    let index = src.root().tree().node_at(&src.text, sign.start);
    let Some(receiver) = psi::prefix(index).filter(|_| index.is(N::IndexExpr)) else { return };
    if receiver.descendants().any(|inner| inner.is(N::Error)) {
        return;
    }
    let (start, end) = receiver.code_range();
    let expression = literal(&src.text[start..end]);
    let statements = if alone(src, index, start, at) { &STATEMENTS[..] } else { &[] };
    let wraps = EXPRESSIONS.iter().filter(|wrap| wrap.key != "par" || !receiver.is(N::ParenExpr));
    for template in statements.iter().chain(wraps) {
        let mut item = CompletionItem::new(template.key, CompletionKind::Keyword);
        item.detail = Some(template.shown.to_owned());
        item.insert_text = template.body.replace('%', &expression);
        item.snippet = true;
        item.replace = Some(listing.doc.range(start, at));
        item.sort_text = Some(format!("{SORT}{}", template.key));
        listing.push(item);
    }
}

#[cfg(test)]
mod tests {
    use super::SORT;
    use crate::editor::EditorFeature;
    use crate::editor::completion::tests::{MAIN, MANIFEST};
    use crate::feature::Feature;
    use crate::testing::Fixture;
    use openidea_plugin::providers::{CompletionItem, CompletionKind};

    /// The templates listed at the `<caret>` of `text`, and the text without the mark.
    fn listed(text: &str) -> (String, Vec<CompletionItem>) {
        let fx = Fixture::new(&[MANIFEST, (MAIN, text)]);
        let feature = EditorFeature::default();
        fx.index(&feature);
        let (doc, at) = fx.caret(MAIN);
        let items = feature.completion(&fx.cx(&doc), at, None);
        let templates = items.into_iter().filter(|item| item.sort_text.as_deref().is_some_and(|s| s.starts_with(SORT)));
        (doc.text().to_owned(), templates.collect())
    }

    fn keys(text: &str) -> Vec<String> {
        listed(text).1.into_iter().map(|item| item.label).collect()
    }

    /// The text once the template `key` is accepted, its stops left as they are written.
    fn expanded(text: &str, key: &str) -> String {
        let (text, items) = listed(text);
        let item = items.iter().find(|item| item.label == key).unwrap_or_else(|| panic!("no template {key}"));
        assert!(item.snippet && item.kind == Some(CompletionKind::Keyword));
        let range = item.replace.expect("what it replaces");
        let lines = crate::text::LineIndex::new(&text);
        let (start, end) = (lines.offset(&text, range.start), lines.offset(&text, range.end));
        format!("{}{}{}", &text[..start], item.insert_text, &text[end..])
    }

    #[test]
    fn an_expression_alone_on_its_line_becomes_a_statement() {
        assert_eq!(
            keys("local count = 1\ncount.<caret>"),
            [
                "decrease",
                "for",
                "for_i",
                "for_p",
                "if",
                "if_nil",
                "if_not",
                "if_not_nil",
                "increase",
                "par",
                "print",
                "return",
                "tonumber",
                "tostring",
                "var",
            ]
        );
        assert_eq!(expanded("count.if<caret>", "if"), "if count then\n\t$0\nend");
        assert_eq!(expanded("count.i<caret>\nprint(1)", "if_not"), "if not count then\n\t$0\nend\nprint(1)");
        assert_eq!(expanded("Config.items.<caret>", "if_nil"), "if Config.items == nil then\n\t$0\nend");
        assert_eq!(expanded("get(1).value.<caret>", "if_not_nil"), "if get(1).value ~= nil then\n\t$0\nend");
        assert_eq!(expanded("list.count.for<caret>", "for"), "for i = 1, list.count do\n\t$0\nend");
        assert_eq!(expanded("list.for<caret>", "for_i"), "for i, v in ipairs(list) do\n\t$0\nend");
        assert_eq!(expanded("list.for<caret>", "for_p"), "for k, v in pairs(list) do\n\t$0\nend");
        assert_eq!(expanded("do\n    total.<caret>\nend", "return"), "do\n    return total\nend");
        assert_eq!(expanded("total.<caret>", "print"), "print(total)");
        assert_eq!(expanded("t.n.<caret>", "increase"), "t.n = t.n + ${1:1}");
        assert_eq!(expanded("t.n.<caret>", "decrease"), "t.n = t.n - ${1:1}");
        assert_eq!(expanded("get().<caret>", "var"), "local ${1:inst} = get()$0");
        // The next line is another statement, even where the parser reads it as the rest of this one.
        assert_eq!(expanded("total.<caret>\nfoo()", "print"), "print(total)\nfoo()");
    }

    #[test]
    fn an_expression_inside_a_statement_is_only_wrapped() {
        assert_eq!(keys("local v = cfg.<caret>"), ["par", "tonumber", "tostring"]);
        assert_eq!(expanded("local v = cfg.name.<caret>", "tostring"), "local v = tostring(cfg.name)");
        assert_eq!(expanded("print(a + b.<caret>)", "par"), "print(a + (b)$0)");
        assert_eq!(expanded("x = '10'.<caret>", "tonumber"), "x = tonumber('10')");
        assert_eq!(keys("x = (a + b).<caret>"), ["tonumber", "tostring"], "already between parentheses");
        assert_eq!(keys("total.<caret> = 1"), ["par", "tonumber", "tostring"], "a field is being named");
        assert!(keys("total:<caret>").is_empty(), "a method is called");
        assert!(keys("function Shop.<caret>").is_empty(), "a function is being named");
    }

    #[test]
    fn what_the_expression_is_written_with_stays_as_it_is() {
        assert_eq!(expanded("price('$5}').<caret>", "print"), "print(price('\\$5\\}'))");
    }
}
