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
    assert_eq!(titles("local n = 1 <caret>+ 2 * 3"), ["Compute constant value of 1 + 2 * 3", "Create type annotation"]);
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

#[test]
fn a_parameter_and_a_local_get_their_annotation() {
    assert_eq!(
        applied("local function open(do<caret>or, side) end", "Create parameter annotation"),
        "---@param door any\nlocal function open(door, side) end"
    );
    assert_eq!(
        applied(
            "--- Opens.\n---@param side string\nlocal function open(door<caret>, side) end",
            "Create parameter annotation"
        ),
        "--- Opens.\n---@param side string\n---@param door any\nlocal function open(door, side) end"
    );
    assert_eq!(
        applied("do\n    f(function(<caret>item) end)\nend", "Create parameter annotation"),
        "do\n    ---@param item any\n    f(function(item) end)\nend"
    );
    let documented = "---@param door number\nlocal function open(do<caret>or) end";
    assert!(!titles(documented).contains(&"Create parameter annotation".to_owned()));
    assert_eq!(applied("local na<caret>me = 'x'", "Create type annotation"), "---@type string\nlocal name = 'x'");
    assert_eq!(applied("local count = <caret>1", "Create type annotation"), "---@type number\nlocal count = 1");
    assert_eq!(applied("local what<caret>", "Create type annotation"), "---@type any\nlocal what");
    let annotated = titles("---@type string\nlocal na<caret>me = 'x'");
    assert_eq!(annotated, ["Rename 'name'…"], "nothing is left to offer but its rename");
}

#[test]
fn a_function_is_documented_from_its_header() {
    assert_eq!(
        applied("function ge<caret>t() return 1 end", "Create return annotation"),
        "---@return number\nfunction get() return 1 end"
    );
    assert_eq!(
        applied("--- Gets.\nlocal function ge<caret>t(a) return a end", "Create return annotation"),
        "--- Gets.\n---@return any\nlocal function get(a) return a end"
    );
    assert_eq!(
        applied("Shop = {}\nfunction Shop.ad<caret>d(item, count) end", "Create LuaDoc"),
        "Shop = {}\n---add\n---@param item any\n---@param count any\nfunction Shop.add(item, count) end"
    );
    let header = titles("Shop = {}\nfunction Shop:clo<caret>se() end");
    assert_eq!(
        header,
        [
            "Create return annotation",
            "Create LuaDoc",
            "Set visibility: private",
            "Set visibility: protected",
            "Set visibility: public",
            "Rename 'close' in the project…",
        ]
    );
    assert_eq!(
        applied("Shop = {}\n    function Shop:clo<caret>se() end", "Set visibility: protected"),
        "Shop = {}\n    ---@protected\n    function Shop:close() end"
    );
    let stated = titles("Shop = {}\n---@private\n---@return number\nfunction Shop:clo<caret>se() return 1 end");
    assert_eq!(stated, ["Rename 'close' in the project…"], "nothing is left to offer but its rename");
    assert_eq!(titles("function get()\n    pri<caret>nt(1)\nend"), Vec::<String>::new(), "inside the body");
    assert!(titles("local get = func<caret>tion() end").iter().all(|title| title == "Create type annotation"));
}

#[test]
fn a_parameter_of_a_method_is_kept_in_a_field() {
    let title = "Create field for parameter";
    assert_eq!(
        applied("function Shop:open(do<caret>or)\nend", title),
        "function Shop:open(door)\n    self.door = door\nend"
    );
    assert_eq!(
        applied("function Shop:open(do<caret>or) end", title),
        "function Shop:open(door)\n    self.door = door\nend"
    );
    assert_eq!(
        applied("do\n\tfunction Shop:open(a, side<caret>)\n\t\tprint(a)\n\tend\nend", title),
        "do\n\tfunction Shop:open(a, side)\n\t\tself.side = side\n\t\tprint(a)\n\tend\nend"
    );
    for kept in
        ["function Shop.open(do<caret>or) end", "function open(do<caret>or) end", "f(function(do<caret>or) end)"]
    {
        assert!(!titles(kept).contains(&title.to_owned()), "{kept}");
    }
}
