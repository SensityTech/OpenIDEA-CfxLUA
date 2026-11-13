use super::*;

const CARET: &str = "<caret>";

fn found(text: &str) -> (String, Option<Renamed>) {
    let offset = text.find(CARET).expect("a caret in the text");
    let text = text.replacen(CARET, "", 1);
    let renamed = at(&Source::new(&text), offset);
    (text, renamed)
}

fn renamed(text: &str, new: &str) -> String {
    let (mut text, renamed) = found(text);
    let renamed = renamed.expect("something to rename at the caret");
    assert_eq!(valid(&renamed.name, new), Ok(()));
    for (start, end, replacement) in edits(&renamed, new).into_iter().rev() {
        text.replace_range(start..end, &replacement);
    }
    text
}

fn nothing(text: &str) {
    assert_eq!(found(text).1, None, "{text}");
}

#[test]
fn a_local_is_renamed_with_every_use() {
    assert_eq!(
        renamed("local <caret>count = 0\ncount = count + 1\nprint(count)", "total"),
        "local total = 0\ntotal = total + 1\nprint(total)"
    );
}

#[test]
fn the_name_and_the_ranges_are_given_in_order() {
    let (_, renamed) = found("local a = 1\nprint(<caret>a, a)");
    assert_eq!(renamed, Some(Renamed { name: "a".to_owned(), ranges: vec![(6, 7), (18, 19), (21, 22)] }));
}

#[test]
fn a_shadowed_local_renames_only_the_one_in_sight() {
    let src = "local a = 1\ndo\n local a = a + 1\n print(a)\nend\nprint(a)";
    let inner = src.replacen("print(a)\nend", "print(<caret>a)\nend", 1);
    assert_eq!(renamed(&inner, "b"), "local a = 1\ndo\n local b = a + 1\n print(b)\nend\nprint(a)");
    let outer = src.replacen("end\nprint(a)", "end\nprint(<caret>a)", 1);
    assert_eq!(renamed(&outer, "b"), "local b = 1\ndo\n local a = b + 1\n print(a)\nend\nprint(b)");
}

#[test]
fn a_parameter_shadowing_a_local_renames_only_the_parameter() {
    assert_eq!(
        renamed("local a = 1\nlocal function f(<caret>a)\n return a\nend\nprint(a)", "x"),
        "local a = 1\nlocal function f(x)\n return x\nend\nprint(a)"
    );
}

#[test]
fn a_documented_parameter_takes_its_param_tag_along() {
    let src = "---@param name string\n---@return string\nlocal function greet(name)\n return name\nend";
    let expected = "---@param who string\n---@return string\nlocal function greet(who)\n return who\nend";
    assert_eq!(renamed(&src.replacen("return name", "return <caret>name", 1), "who"), expected);
    assert_eq!(renamed(&src.replacen("(name)", "(<caret>name)", 1), "who"), expected);
    assert_eq!(renamed(&src.replacen("@param name", "@param <caret>name", 1), "who"), expected);
}

#[test]
fn the_param_tag_follows_a_method_and_leaves_a_nested_function_alone() {
    assert_eq!(
        renamed("---@param x number\nfunction M:f(<caret>x)\n return function(x) return x end\nend", "y"),
        "---@param y number\nfunction M:f(y)\n return function(x) return x end\nend"
    );
}

#[test]
fn a_param_tag_that_names_no_parameter_renames_nothing() {
    nothing("---@param <caret>other string\nlocal function f(name) end");
}

#[test]
fn a_function_declared_ahead_is_the_local() {
    let src = "local f\nfunction f() return 1 end\nprint(f())";
    let expected = "local g\nfunction g() return 1 end\nprint(g())";
    assert_eq!(renamed(&src.replacen("function f", "function <caret>f", 1), "g"), expected);
    assert_eq!(renamed(&src.replacen("local f", "local <caret>f", 1), "g"), expected);
    assert_eq!(renamed(&src.replacen("print(f", "print(<caret>f", 1), "g"), expected);
}

#[test]
fn a_local_function_is_renamed_with_its_recursive_calls() {
    assert_eq!(
        renamed("local function <caret>fact(n)\n return n * fact(n - 1)\nend\nfact(3)", "f"),
        "local function f(n)\n return n * f(n - 1)\nend\nf(3)"
    );
}

#[test]
fn a_numeric_loop_variable_lives_in_its_body_only() {
    assert_eq!(
        renamed("local i = 0\nfor i = i, 10 do\n print(<caret>i)\nend\nprint(i)", "n"),
        "local i = 0\nfor n = i, 10 do\n print(n)\nend\nprint(i)"
    );
}

#[test]
fn a_generic_loop_variable_is_renamed() {
    assert_eq!(
        renamed("for <caret>k, v in pairs(t) do\n print(k, v)\nend", "key"),
        "for key, v in pairs(t) do\n print(key, v)\nend"
    );
    assert_eq!(
        renamed("---@param v number\nfor k, v in pairs(t) do\n print(k, <caret>v)\nend", "value"),
        "---@param value number\nfor k, value in pairs(t) do\n print(k, value)\nend"
    );
}

#[test]
fn a_local_read_from_a_nested_function_is_renamed_there() {
    assert_eq!(
        renamed(
            "local <caret>count = 0\nlocal function add()\n count = count + 1\n return function() return count end\nend",
            "n"
        ),
        "local n = 0\nlocal function add()\n n = n + 1\n return function() return n end\nend"
    );
}

#[test]
fn a_local_with_an_attribute_is_renamed() {
    assert_eq!(renamed("local <caret>max <const> = 3\nprint(max)", "limit"), "local limit <const> = 3\nprint(limit)");
}

#[test]
fn a_goto_renames_the_label_written_after_it() {
    let src = "for i = 1, 3 do\n if i == 2 then goto continue end\n print(i)\n ::continue::\nend";
    let expected = "for i = 1, 3 do\n if i == 2 then goto skip end\n print(i)\n ::skip::\nend";
    assert_eq!(renamed(&src.replacen("goto continue", "goto <caret>continue", 1), "skip"), expected);
    assert_eq!(renamed(&src.replacen("::continue", "::<caret>continue", 1), "skip"), expected);
}

#[test]
fn a_label_takes_every_goto_that_jumps_to_it_and_no_other() {
    let src = "for i = 1, 3 do\n for j = 1, 3 do\n  if j then goto continue end\n  ::continue::\n end\n if i then goto continue end\n goto continue\n ::continue::\nend\nlocal function f()\n goto continue\nend";
    let expected = "for i = 1, 3 do\n for j = 1, 3 do\n  if j then goto continue end\n  ::continue::\n end\n if i then goto next end\n goto next\n ::next::\nend\nlocal function f()\n goto continue\nend";
    assert_eq!(
        renamed(&src.replacen(" ::continue::\nend\nlocal", " ::<caret>continue::\nend\nlocal", 1), "next"),
        expected
    );
    assert_eq!(renamed(&src.replacen("goto continue\n ::", "goto <caret>continue\n ::", 1), "next"), expected);
}

#[test]
fn a_label_inside_a_function_is_renamed_there() {
    assert_eq!(
        renamed("local function f()\n goto <caret>done\n ::done::\nend\n::done::", "out"),
        "local function f()\n goto out\n ::out::\nend\n::done::"
    );
}

#[test]
fn a_goto_to_no_label_renames_nothing() {
    nothing("goto <caret>nowhere");
}

#[test]
fn the_caret_at_the_start_in_the_middle_or_just_after_the_name() {
    let expected = "local b = 1\nprint(b + 2)";
    assert_eq!(renamed("local <caret>abc = 1\nprint(abc + 2)", "b"), expected);
    assert_eq!(renamed("local a<caret>bc = 1\nprint(abc + 2)", "b"), expected);
    assert_eq!(renamed("local abc<caret> = 1\nprint(abc + 2)", "b"), expected);
    assert_eq!(renamed("local abc = 1\nprint(abc<caret>+2)", "b"), "local b = 1\nprint(b+2)");
}

#[test]
fn the_caret_away_from_any_name_renames_nothing() {
    nothing("local abc = 1\nprint(abc +<caret> 2)");
    nothing("local abc = <caret>1");
    nothing("-- <caret>abc\nlocal abc = 1");
}

#[test]
fn a_global_a_field_or_a_method_is_not_renamed_here() {
    nothing("<caret>count = 1\nprint(count)");
    nothing("count = 1\nprint(<caret>count)");
    nothing("function <caret>run() end");
    nothing("<caret>print(1)");
    nothing("local t = {}\nt.<caret>field = 1");
    nothing("local t = { <caret>field = 1 }");
    nothing("local t = {}\nfunction t:<caret>run() end");
    nothing("local t = {}\nt:<caret>run()");
    nothing("function M:run()\n return <caret>self\nend");
}

#[test]
fn a_new_name_must_be_a_lua_identifier_that_is_no_keyword_and_differs() {
    assert_eq!(valid("a", "b_2"), Ok(()));
    assert_eq!(valid("a", "_"), Ok(()));
    assert_eq!(valid("a", "continue"), Ok(()));
    assert_eq!(valid("a", ""), Err("Enter a new name".to_owned()));
    assert_eq!(valid("a", "a"), Err("The name is unchanged".to_owned()));
    for keyword in ["end", "goto", "nil", "local", "function"] {
        assert_eq!(valid("a", keyword), Err(format!("'{keyword}' is a keyword")));
    }
    for wrong in ["1a", "a-b", "a b", " a", "a.b", "é", "`a`"] {
        assert_eq!(valid("a", wrong), Err(format!("'{wrong}' is not a valid Lua identifier")));
    }
}

#[test]
fn edits_replace_each_range_with_the_new_name_in_order() {
    let (_, renamed) = found("local <caret>a = 1\nprint(a)");
    let renamed = renamed.expect("a local");
    assert_eq!(edits(&renamed, "bb"), [(6, 7, "bb".to_owned()), (18, 19, "bb".to_owned())]);
}

/// What the fix offered at the caret asks, and the text once each answer of `answers` is given in turn.
fn asked_at(text: &str, answers: &[Option<&str>]) -> (Vec<String>, Option<String>) {
    let offset = text.find(CARET).expect("a caret in the text");
    let mut text = text.replacen(CARET, "", 1);
    let src = Source::new(&text);
    let offered = fix(&src, offset, |name| format!("Rename '{name}'…")).expect("a fix");
    assert!(offered.edits.is_empty(), "its edits wait for the name");
    let mut questions = Vec::new();
    let mut answers = answers.iter();
    let mut ask = |question: &str, value: &str| {
        questions.push(format!("{question} [{value}]"));
        answers.next().copied().flatten().map(str::to_owned)
    };
    let edits = asked(&src, offered.asked.as_deref().expect("what it is asked again with"), &mut ask);
    let renamed = edits.map(|edits| {
        for (start, end, replacement) in edits.into_iter().rev() {
            text.replace_range(start..end, &replacement);
        }
        text
    });
    (questions, renamed)
}

#[test]
fn the_name_is_asked_once_the_fix_is_chosen() {
    let text = "local co<caret>unt = 0\nprint(count)";
    let (questions, renamed) = asked_at(text, &[Some(" total ")]);
    assert_eq!(questions, ["Rename 'count' to: [count]"]);
    assert_eq!(renamed.as_deref(), Some("local total = 0\nprint(total)"), "what is entered is trimmed");
    assert_eq!(asked_at(text, &[None]).1, None, "the question was dismissed");
    assert_eq!(asked_at(text, &[Some("count")]).1, None, "the same name: nothing to rename");
    let (questions, renamed) = asked_at(text, &[Some("end"), Some("2x"), Some("total")]);
    assert_eq!(
        questions,
        [
            "Rename 'count' to: [count]",
            "'end' is a keyword. Rename 'count' to: [end]",
            "'2x' is not a valid Lua identifier. Rename 'count' to: [2x]",
        ],
        "a name that cannot be is asked again, with the reason and what was entered"
    );
    assert_eq!(renamed.as_deref(), Some("local total = 0\nprint(total)"));
    assert_eq!(asked_at(text, &[Some("end"), None]).1, None);
}

#[test]
fn a_fix_offered_on_a_text_that_changed_renames_nothing() {
    let src = Source::new("local count = 0\nprint(count)");
    let offered = fix(&src, 7, |_| "Rename".to_owned()).expect("a fix");
    assert_eq!(offered.title, "Rename");
    let data = offered.asked.expect("what it is asked again with");
    let mut never = |_: &str, _: &str| -> Option<String> { panic!("nothing is asked") };
    assert_eq!(asked(&Source::new("local other = 0\nprint(other)"), &data, &mut never), None);
    assert_eq!(asked(&Source::new("print(1)"), &data, &mut never), None);
    assert_eq!(asked(&src, "something else", &mut never), None);
    assert!(fix(&Source::new("Config = 1"), 2, |_| String::new()).is_none(), "a global is not renamed here");
}
