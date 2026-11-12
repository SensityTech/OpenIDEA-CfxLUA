use crate::editor::EditorFeature;
use crate::feature::{Feature, Fix};
use crate::testing::Fixture;

const MANIFEST: (&str, &str) =
    ("shop/fxmanifest.lua", "fx_version 'cerulean'\ngame 'gta5'\nshared_script 'shared/*.lua'\n");
const MAIN: &str = "shop/shared/main.lua";
const END: &str = "<end>";

/// The text without its marks, and what the editor offers there: at the `<caret>`, or for the selection that
/// goes from it to `<end>`.
fn offered(text: &str) -> (String, Vec<Fix>) {
    let end = text.find(END);
    let marked = text.replacen(END, "", 1);
    let fx = Fixture::new(&[MANIFEST, (MAIN, &marked)]);
    let feature = EditorFeature::default();
    fx.index(&feature);
    let (doc, at) = fx.caret(MAIN);
    let end = end.map_or(at, |end| end - "<caret>".len());
    let found = feature.intentions(&fx.cx(&doc), at, end);
    assert!(found.iter().all(|intention| intention.kind == "refactor"));
    (doc.text().to_owned(), found.into_iter().map(|intention| intention.fix).collect())
}

fn titles(text: &str) -> Vec<String> {
    offered(text).1.into_iter().map(|fix| fix.title).collect()
}

/// The text once the rewrite `title` is applied.
fn applied(text: &str, title: &str) -> String {
    let (mut text, fixes) = offered(text);
    let names: Vec<&str> = fixes.iter().map(|fix| fix.title.as_str()).collect();
    let fix = fixes.iter().find(|fix| fix.title == title).unwrap_or_else(|| panic!("no '{title}' in {names:?}"));
    let mut edits = fix.edits.clone();
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));
    for (start, end, with) in edits {
        text.replace_range(start..end, &with);
    }
    text
}

#[test]
fn a_call_takes_or_leaves_its_parentheses() {
    assert_eq!(applied("print <caret>\"x\"", "Append call paren"), "print(\"x\")");
    assert_eq!(applied("f<caret>{ a = 1 }", "Append call paren"), "f({ a = 1 })");
    assert_eq!(applied("obj:method 'x'<caret>", "Append call paren"), "obj:method('x')");
    assert_eq!(applied("print(<caret>\"x\")", "Remove call paren"), "print \"x\"");
    assert_eq!(applied("f({<caret>})", "Remove call paren"), "f {}");
    assert_eq!(applied("a.b:c(  [[x]]  )<caret>", "Remove call paren"), "a.b:c [[x]]");
    for kept in ["f(<caret>1)", "f(<caret>'a', 'b')", "f(<caret>x)", "f(<caret>)", "local a = <caret>1"] {
        assert!(titles(kept).iter().all(|title| !title.contains("call paren")), "{kept}");
    }
    // The call the caret is in: not the one around it.
    assert_eq!(applied("outer(inner <caret>'x')", "Append call paren"), "outer(inner('x'))");
}

#[test]
fn a_boolean_is_inverted() {
    assert_eq!(applied("local on = tr<caret>ue", "Invert boolean value"), "local on = false");
    assert_eq!(applied("f(false<caret>)", "Invert boolean value"), "f(true)");
    assert!(titles("on = n<caret>il").is_empty());
}

#[test]
fn an_operation_between_constants_is_computed_from_its_operator() {
    let computed = |text: &str| {
        let title = titles(text).into_iter().find(|title| title.starts_with("Compute constant value of "));
        title.map(|title| applied(text, &title))
    };
    assert_eq!(titles("local n = 1 <caret>+ 2 * 3"), ["Compute constant value of 1 + 2 * 3"]);
    assert_eq!(computed("local n = 1 <caret>+ 2 * 3").as_deref(), Some("local n = 7"));
    assert_eq!(computed("local n = 1 + 2 <caret>* 3").as_deref(), Some("local n = 1 + 6"));
    assert_eq!(computed("n = (1 + 2) <caret>* 3").as_deref(), Some("n = 9"));
    assert_eq!(computed("n = 7 <caret>// 2").as_deref(), Some("n = 3"));
    assert_eq!(computed("n = 7 <caret>% 3").as_deref(), Some("n = 1"));
    assert_eq!(computed("n = 1 <caret>/ 4").as_deref(), Some("n = 0.25"));
    assert_eq!(computed("n = 0x10 <caret>- 1.5").as_deref(), Some("n = 14.5"));
    assert_eq!(computed("s = 'a' <caret>.. 'b'").as_deref(), Some("s = 'ab'"));
    assert_eq!(computed("s = \"it's \" <caret>.. 1").as_deref(), Some("s = \"it's 1\""));
    assert_eq!(computed("s = 'it' <caret>.. \"'s\"").as_deref(), Some("s = 'it\\'s'"));
    let hash = crate::lua::joaat::joaat("door");
    assert_eq!(computed("h = `door` <caret>+ 0"), Some(format!("h = {hash}")));
    // `and` and `or` follow the truth of Lua, and say nothing of what is no constant.
    assert_eq!(computed("v = nil <caret>or 2").as_deref(), Some("v = 2"));
    assert_eq!(computed("v = 1 <caret>or x").as_deref(), Some("v = 1"));
    assert_eq!(computed("v = false <caret>and x").as_deref(), Some("v = false"));
    assert_eq!(computed("v = true <caret>and f()").as_deref(), Some("v = f()"));
    assert_eq!(computed("v = 0 <caret>and 'zero'").as_deref(), Some("v = 'zero'"), "0 is true in Lua");
    for kept in
        ["v = x <caret>or 1", "v = x <caret>and 1", "v = 1 <caret>/ 0", "v = 1 <caret>< 2", "v = 'a' <caret>+ 1"]
    {
        assert_eq!(computed(kept), None, "{kept}");
    }
    assert_eq!(computed("n = <caret>1 + 2"), None, "on an operand the caret is about that operand");
}

#[test]
fn the_selected_statements_are_marked_as_a_region() {
    let text = "local a = 1\n<caret>local b = 2\nprint(b)<end>\nprint(a)";
    let surrounds = |text: &str| -> Vec<String> {
        titles(text).into_iter().filter(|title| title.starts_with("Surround with")).collect()
    };
    assert_eq!(surrounds(text), ["Surround with: Lua Region --region", "Surround with: Lua Region --{{{"]);
    assert_eq!(
        applied(text, "Surround with: Lua Region --region"),
        "local a = 1\n--region description\nlocal b = 2\nprint(b)\n--endregion\nprint(a)"
    );
    assert_eq!(
        applied(
            "function f()\n    local a = 1\n    pri<caret>nt(a)\n    return<end> a\nend",
            "Surround with: Lua Region --{{{"
        ),
        "function f()\n    local a = 1\n    --{{{ description\n    print(a)\n    return a\n    --}}}\nend",
        "the statements the selection touches, with their indentation"
    );
    assert_eq!(
        applied("if a then\n    f<caret>()\nend\ng(<end>)", "Surround with: Lua Region --region"),
        "--region description\nif a then\n    f()\nend\ng()\n--endregion",
        "statements of one block: the one around the deeper end of the selection"
    );
    assert!(surrounds("local a = 1\n<caret>\n<end>print(a)").is_empty(), "nothing but white space");
    assert!(surrounds("local a = 1\n<caret>print(a)").is_empty(), "no selection");
}
