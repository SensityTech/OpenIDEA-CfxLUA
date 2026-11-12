use super::*;
use crate::lua::token::Tokens;

fn style() -> Style {
    Style::new(4, true)
}

/// What is off by default turned on: the simple blocks kept on their line and every alignment.
fn every_option() -> Style {
    Style {
        keep_simple_blocks: true,
        align_arguments: true,
        align_consecutive_assignments: true,
        align_table_field_assign: true,
        ..style()
    }
}

fn done(text: &str) -> String {
    formatted(&Source::new(text), &style())
}

fn with(change: impl Fn(&mut Style), text: &str) -> String {
    let mut style = style();
    change(&mut style);
    formatted(&Source::new(text), &style)
}

/// The tokens of a text that are no white space, a `;` left out where `semicolons` is not asked for. The
/// lines of a documentation comment are read without their indent, which the formatter rewrites.
fn code(text: &str, semicolons: bool) -> Vec<String> {
    let src = Source::new(text);
    let tokens: &Tokens = src.root().tree().tokens();
    let kept = |index: &usize| tokens.kind(*index) != T::Whitespace && (semicolons || tokens.kind(*index) != T::Semi);
    let read = |index: usize| match tokens.kind(index) {
        T::DocComment => tokens.text(&src.text, index).lines().map(str::trim_start).collect::<Vec<_>>().join("\n"),
        _ => tokens.text(&src.text, index).to_owned(),
    };
    (0..tokens.len()).filter(kept).map(read).collect()
}

/// `codeStyle/preview/preview.lua.template`, the sample of the Code Style page of the JetBrains plugin.
const PREVIEW: &str = "local inlineEmmy = { name = \"Emmy\", age = 7, longLongProperty = 123 }
local emmy = {
    name = \"Emmy\",
    age = 7,
    longLongProperty = 123
}
local a = 1
local b, c = 2, \"3\"
local var = 1 -- a short comment
--- doc comment
--- @param par1 Par1Type @some strings
function var:testFun(par1, par2, par3, par4, par5)
   print('hello')
   return self.len + 2
end

var:testFun('par1Value', 'par2Value', 'par3Value', 'par4Value', 'par5Value')

function globalFun()
   return \"string\" .. var
end

if true then return 123 end

if true then print(\"123\") return 123 end
";

#[test]
fn the_sample_of_the_code_style_page() {
    let bodies = PREVIEW.replace("   print('hello')\n   return self", "    print('hello')\n    return self");
    let bodies = bodies.replace("   return \"string\"", "    return \"string\"");
    let blocks = bodies.replace("if true then return 123 end", "if true then\n    return 123\nend");
    let blocks = blocks
        .replace("if true then print(\"123\") return 123 end", "if true then\n    print(\"123\")\n    return 123\nend");
    assert_eq!(done(PREVIEW), blocks);
    // `KEEP_SIMPLE_BLOCKS_IN_ONE_LINE` keeps the block that holds one statement, not the one that holds two.
    let kept = bodies
        .replace("if true then print(\"123\") return 123 end", "if true then\n    print(\"123\")\n    return 123\nend");
    assert_eq!(with(|style| style.keep_simple_blocks = true, PREVIEW), kept);
}

#[test]
fn spaces_follow_the_rules_of_the_spacing_builder() {
    assert_eq!(done("local   a=1"), "local a = 1");
    assert_eq!(done("a   =   1\nb+=2\nc  -=  3"), "a = 1\nb += 2\nc -= 3");
    assert_eq!(done("f( a ,b )\ng ()\nh( )"), "f(a, b)\ng()\nh()");
    assert_eq!(done("x=a+b*c-d\ny=a..b\nz=a<b"), "x = a + b * c - d\ny = a .. b\nz = a < b");
    assert_eq!(done("x=-a\ny = - a\nz=#t\nw = not  a"), "x = -a\ny = -a\nz = #t\nw = not a");
    assert_eq!(done("x = a  and  b  or  c"), "x = a and b or c");
    assert_eq!(done("t={1,2,3}\nu={a=1 ,b=2}"), "t = { 1, 2, 3 }\nu = { a = 1, b = 2 }");
    assert_eq!(done("t [ 1 ] = 2\nu [ 'k' ] [ 2 ] = 3"), "t[1] = 2\nu['k'][2] = 3");
    assert_eq!(done("a : b ( )\nc:d( 1 )"), "a:b()\nc:d(1)");
    assert_eq!(done("return   a ,b"), "return a, b");
    assert_eq!(done("local  function  f(a,b)\nend"), "local function f(a, b)\nend");
    assert_eq!(
        done("for i=1,10 do\nend\nfor k,v in pairs( t ) do\nend"),
        "for i = 1, 10 do\nend\nfor k, v in pairs(t) do\nend"
    );
    assert_eq!(done("goto   done\nlocal a <const> = 1"), "goto done\nlocal a <const> = 1");
}

#[test]
fn a_call_without_parentheses_takes_one_space() {
    assert_eq!(done("f\"x\"\ng  'y'\nh{1}\nrequire[[m]]"), "f \"x\"\ng 'y'\nh { 1 }\nrequire [[m]]");
    assert_eq!(done("f  (1)\ng\n(2)"), "f(1)\ng(2)", "no line break between a function and its arguments");
}

#[test]
fn what_no_rule_names_stays_as_it_is_written() {
    // Nothing is said of a dot, of the braces of an empty table, of a name and the parameters after it.
    assert_eq!(done("x = a . b\ny = a.b"), "x = a . b\ny = a.b");
    assert_eq!(done("t = { }\nu = {}"), "t = { }\nu = {}");
    assert_eq!(done("function f (a)\nend\nfunction g(a)\nend"), "function f (a)\nend\nfunction g(a)\nend");
    assert_eq!(done("a = 1   -- one\nb = 2 --[[ two ]]"), "a = 1   -- one\nb = 2 --[[ two ]]");
}

#[test]
fn a_label_keeps_the_spaces_it_is_written_with() {
    // The JetBrains plugin writes `:: done ::`: `::` is one of its keyword tokens, which take a space.
    assert_eq!(done("::done::\ngoto   done"), "::done::\ngoto done");
    assert_eq!(done(":: done ::\n::  other::"), ":: done ::\n::  other::");
    assert_eq!(
        done("for i = 1, 2 do\nif i then goto continue end\n::continue::\nend"),
        "for i = 1, 2 do\n    if i then\n        goto continue\n    end\n    ::continue::\nend"
    );
}

#[test]
fn a_block_is_written_on_its_own_lines() {
    assert_eq!(done("if a then b() end"), "if a then\n    b()\nend");
    assert_eq!(
        done("if a then b() elseif c then d() else e() end"),
        "if a then\n    b()\nelseif c then\n    d()\nelse\n    e()\nend"
    );
    assert_eq!(done("while x do x=x-1 end"), "while x do\n    x = x - 1\nend");
    assert_eq!(done("do local x=1 end"), "do\n    local x = 1\nend");
    assert_eq!(done("for i=1,2 do print(i) end"), "for i = 1, 2 do\n    print(i)\nend");
    assert_eq!(done("function f(a) return a end"), "function f(a)\n    return a\nend");
    assert_eq!(done("local f=function(a)return a end"), "local f = function(a)\n    return a\nend");
    assert_eq!(done("f(function() end)"), "f(function()\nend)");
    // Nothing breaks the line after `repeat` nor before `until`.
    assert_eq!(done("repeat x=x+1 until x>3"), "repeat x = x + 1 until x > 3");
    assert_eq!(done("repeat\nx=x+1\nuntil x>3"), "repeat\n    x = x + 1\nuntil x > 3");
    // A comment after `then` goes to the body: the line breaks after `then` whatever follows.
    assert_eq!(done("if a then -- why\nb()\nend"), "if a then\n    -- why\n    b()\nend");
}

#[test]
fn a_simple_block_may_stay_on_one_line() {
    let kept = |text: &str| with(|style| style.keep_simple_blocks = true, text);
    assert_eq!(kept("if a then   return   end"), "if a then return end");
    assert_eq!(kept("f(function()  end)"), "f(function() end)");
    assert_eq!(kept("for i=1,2 do print(i) end"), "for i = 1, 2 do print(i) end");
    assert_eq!(kept("if a then\n  return\nend"), "if a then\n    return\nend", "its line breaks are kept");
    assert_eq!(kept("if a then b() c() end"), "if a then\n    b()\n    c()\nend");
    assert_eq!(kept("if a then b() else c() end"), "if a then b() else c() end");
}

#[test]
fn statements_stand_on_their_lines_with_one_blank_line_at_most() {
    assert_eq!(done("a = 1 b = 2"), "a = 1\nb = 2");
    assert_eq!(done("a = 1\n\n\n\nb = 2"), "a = 1\n\nb = 2");
    assert_eq!(done("\n\n  a = 1\n"), "a = 1\n", "nothing before the first token, the end of the file as it is");
    assert_eq!(done("a = 1  \n   \nb = 2"), "a = 1\n\nb = 2", "no white space at the end of a line");
    // Two blank lines are kept where the code style keeps them: after `then`, before `end`.
    assert_eq!(done("if a then\n\n\n\n    b()\n\n\n\nend"), "if a then\n\n\n    b()\n\n\nend");
    // Nothing is said between a comment and a statement: their blank lines stay.
    assert_eq!(done("a = 1\n\n\n\n-- c\n\n\n\nb = 2"), "a = 1\n\n\n\n-- c\n\n\n\nb = 2");
    assert_eq!(done("x =\n    1\ny\n= 2"), "x = 1\ny = 2", "no line break on either side of `=`");
}

#[test]
fn a_body_is_indented_from_the_line_its_block_starts_on() {
    assert_eq!(done("if a then\nif b then\nc()\nend\nend"), "if a then\n    if b then\n        c()\n    end\nend");
    assert_eq!(
        done("CreateThread(function()\nwhile true do\nWait(0)\nend\nend)"),
        "CreateThread(function()\n    while true do\n        Wait(0)\n    end\nend)"
    );
    assert_eq!(
        done("local t = {\na = 1,\n      b = {\n  c = 2,\n     },\n}"),
        "local t = {\n    a = 1,\n    b = {\n        c = 2,\n    },\n}"
    );
    assert_eq!(
        done("RegisterNetEvent('a', function(x)\n  if x then\n print(x)\n  end\nend)"),
        "RegisterNetEvent('a', function(x)\n    if x then\n        print(x)\n    end\nend)"
    );
    assert_eq!(done("    -- note\n        a = 1"), "-- note\na = 1");
    assert_eq!(
        done("do\n-- note\n        --[[ long ]]\na = 1\nend"),
        "do\n    -- note\n    --[[ long ]]\n    a = 1\nend"
    );
    let two = Style::new(2, true);
    assert_eq!(formatted(&Source::new("if a then\nb()\nend"), &two), "if a then\n  b()\nend");
    let tabs = Style::new(4, false);
    assert_eq!(
        formatted(&Source::new("if a then\nif b then\nc()\nend\nend"), &tabs),
        "if a then\n\tif b then\n\t\tc()\n\tend\nend"
    );
}

#[test]
fn what_continues_a_line_is_indented_twice() {
    assert_eq!(done("local x = a +\nb"), "local x = a +\n        b");
    assert_eq!(done("local x = a\n+ b\n+ c"), "local x = a\n        + b\n        + c");
    assert_eq!(done("local x = (\na\n)"), "local x = (\n        a\n)");
    assert_eq!(done("f(a,\nb,\nc)"), "f(a,\n        b,\n        c)");
    assert_eq!(done("f(\na,\nb\n)"), "f(\n        a,\n        b\n)");
    // A function given on its own line: its body is counted from that line.
    assert_eq!(done("f(a,\nfunction()\nx()\nend)"), "f(a,\n        function()\n            x()\n        end)");
    // Nothing indents the values of a list: they start where their statement does.
    assert_eq!(done("return a,\n    b"), "return a,\nb");
}

#[test]
fn parameters_and_the_dots_of_a_chain_are_aligned() {
    assert_eq!(done("function f(a,\nb,\n    c)\nend"), "function f(a,\n           b,\n           c)\nend");
    assert_eq!(
        with(|style| style.align_parameters = false, "function f(a,\nb)\nend"),
        "function f(a,\n        b)\nend"
    );
    assert_eq!(with(|style| style.align_arguments = true, "f(a,\nb,\n        c)"), "f(a,\n  b,\n  c)");
    assert_eq!(done("obj:foo()\n:bar()\n        :baz()"), "obj:foo()\n   :bar()\n   :baz()");
    assert_eq!(done("local x = a.b\n.c"), "local x = a.b\n           .c");
    // A dot that does not start its line is lined up as well, and the first dot moves where a later one
    // stands further right: what the alignment of the JetBrains plugin asks the platform for.
    assert_eq!(done("Builder.new(a, {\n    k = 1,\n}).add(x)"), "Builder.new(a, {\n    k = 1,\n})     .add(x)");
    assert_eq!(
        done("Obj.method(a, function()\n    return 1\nend).other(b)"),
        "Obj .method(a, function()\n    return 1\nend).other(b)"
    );
    // Fields that follow the brace on its line: the next ones line up with the first one.
    assert_eq!(done("local t = { a = 1,\nb = 2 }"), "local t = { a = 1,\n            b = 2 }");
}

#[test]
fn an_alignment_that_cannot_be_reached_is_given_up() {
    // The last dot stands right of the first one, and moving the first one moves it as much. IntelliJ leaves
    // the first dot moved and moves it again at each pass: here the dots are left where they are written.
    let arguments = |text: &str| with(|style| style.align_arguments = true, text);
    let chain = arguments("Chain.first(a)\n    .second({\n        k = 1,\n    }).third(b)");
    assert_eq!(chain, "Chain.first(a)\n        .second({\n                    k = 1,\n                }).third(b)");
    assert_eq!(arguments(&chain), chain);
    let call = arguments("local value = name.method(first,\n    second).other()");
    assert_eq!(call, "local value = name.method(first,\n                          second).other()");
    assert_eq!(arguments(&call), call);
}

#[test]
fn the_equal_signs_are_aligned_where_asked() {
    let fields = |text: &str| with(|style| style.align_table_field_assign = true, text);
    assert_eq!(
        fields("local t = {\n    a = 1,\n    long = 2,\n    b = 3,\n}"),
        "local t = {\n    a    = 1,\n    long = 2,\n    b    = 3,\n}"
    );
    assert_eq!(fields("local t = { a = 1, long = 2 }"), "local t = { a = 1, long = 2 }", "one line: nothing to align");
    let assignments = |text: &str| with(|style| style.align_consecutive_assignments = true, text);
    assert_eq!(assignments("local a = 1\nlocal long = 2\nx = 3"), "local a    = 1\nlocal long = 2\nx          = 3");
    assert_eq!(
        assignments("local a = 1\nprint(a)\nlocal long = 2"),
        "local a = 1\nprint(a)\nlocal long = 2",
        "another statement between them: two groups"
    );
    assert_eq!(done("local a = 1\nlocal long = 2"), "local a = 1\nlocal long = 2");
}

#[test]
fn the_spacing_options() {
    assert_eq!(with(|style| style.space_inside_inline_table = false, "t = { 1, 2 }"), "t = {1, 2}");
    assert_eq!(with(|style| style.space_after_field_sep = false, "t = { 1, 2 }"), "t = { 1,2 }");
    assert_eq!(with(|style| style.space_around_binary_operator = false, "x = a + b * c"), "x = a+b*c");
    assert_eq!(
        with(|style| style.space_around_binary_operator = false, "x = a and b or 1 .. 2"),
        "x = a and b or 1 .. 2",
        "words keep their spaces, and a number is not glued to the dots"
    );
    assert_eq!(with(|style| style.space_around_assignment = false, "a = 1\nb += 2"), "a=1\nb+=2");
    assert_eq!(with(|style| style.space_before_comma = true, "f(a,b)"), "f(a , b)");
    assert_eq!(with(|style| style.space_after_comma = false, "f(a, b)"), "f(a,b)");
}

#[test]
fn a_needless_semicolon_is_taken_out() {
    assert_eq!(done("a = 1; b = 2;"), "a = 1\nb = 2");
    assert_eq!(done("a=1;b=2"), "a = 1\nb = 2");
    assert_eq!(done("a = 1;;;\nb = 2 ;"), "a = 1\nb = 2");
    assert_eq!(done("function f() return; end"), "function f()\n    return\nend");
    assert_eq!(done("a = 1; -- why"), "a = 1 -- why");
    // The JetBrains plugin deletes these two as well, which breaks the table and changes what is called.
    assert_eq!(done("local t = {1; 2}"), "local t = { 1; 2 }");
    assert_eq!(done("local a = f; (g or h)()"), "local a = f;\n(g or h)()");
    assert_eq!(with(|style| style.remove_semicolons = false, "a = 1 ; b = 2;"), "a = 1;\nb = 2;");
    assert_eq!(done("a = 1;\nlocal = 2"), "a = 1;\nlocal = 2", "a file that does not parse keeps them");
}

#[test]
fn the_tokens_stay_what_they_are() {
    // Glued, each of these would read as something else: a comment, a long string, another number.
    assert_eq!(done("x = - - y"), "x = - -y");
    assert_eq!(done("x = a - - b"), "x = a - -b");
    assert_eq!(done("t[ [[k]] ] = 1"), "t[ [[k]]] = 1");
    assert_eq!(done("x = 1 .. 2"), "x = 1 .. 2");
    // A comment that ends its line keeps its line break, whatever a rule asks for.
    assert_eq!(done("local x -- why\n= 1"), "local x -- why\n= 1");
    assert_eq!(done("f -- why\n(1)"), "f -- why\n(1)");
    assert_eq!(done("f --[[ why ]] (1)"), "f --[[ why ]](1)");
    for text in [PREVIEW, "a=1;b=2;;\nlocal t={1;2,}\nreturn;", "x = - - y\nt[ [[k]] ] = 1\n--- d\n   --- e\nlocal z"] {
        assert_eq!(code(&done(text), false), code(text, false), "{text}");
    }
}

#[test]
fn a_documentation_comment_has_its_lines_where_its_first_one_starts() {
    assert_eq!(
        done("if a then\n--- One.\n      --- Two.\n   local x = 1\nend"),
        "if a then\n    --- One.\n    --- Two.\n    local x = 1\nend"
    );
    assert_eq!(done("  --- One.\n      ---@type number\nlocal x = 1"), "--- One.\n---@type number\nlocal x = 1");
    assert_eq!(
        done("local t = {\n--- One.\n  --- Two.\n  a = 1,\n}"),
        "local t = {\n    --- One.\n    --- Two.\n    a = 1,\n}"
    );
}

#[test]
fn only_what_touches_the_range_is_formatted() {
    let text = "a=1\nif a then\nb=2\nend\nc=3";
    let src = Source::new(text);
    let (start, end) = (text.find("b=2").expect("the line"), text.find("\nend").expect("its end"));
    let mut edits = format(&src, &style(), Some((start, end))).expect("a layout that settles");
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));
    let mut out = text.to_owned();
    for (from, to, with) in edits {
        out.replace_range(from..to, &with);
    }
    assert_eq!(out, "a=1\nif a then\n    b = 2\nend\nc=3");
    let first = format(&src, &style(), Some((0, 0)));
    assert_eq!(first, Ok(vec![(1, 1, " ".to_owned()), (2, 2, " ".to_owned())]), "the line the caret is on");
}

#[test]
fn many_edits_are_joined_into_fewer_that_write_the_same() {
    let text = "a=1\nif a then b=2 end\n".repeat(200);
    let src = Source::new(&text);
    let edits = format(&src, &style(), None).expect("a layout that settles");
    assert!(edits.len() > 1_000, "{}", edits.len());
    let apply = |edits: &[Edit]| {
        let mut written = text.clone();
        for (from, to, with) in edits.iter().rev() {
            written.replace_range(*from..*to, with);
        }
        written
    };
    for most in [1, 7, 100, 999] {
        let fewer = joined(&text, edits.clone(), most);
        assert!(fewer.len() <= most && !fewer.is_empty(), "{} for {most}", fewer.len());
        assert!(fewer.windows(2).all(|pair| pair[0].1 <= pair[1].0), "in the order of the text");
        assert_eq!(apply(&fewer), apply(&edits), "{most}");
    }
    assert_eq!(joined(&text, edits.clone(), edits.len()), edits, "nothing to join under the limit");
    assert_eq!(apply(&edits), formatted(&src, &style()));
}

#[test]
fn a_line_is_as_deep_as_it_is_long() {
    // Nothing is nested in these: the tree of a sum, of a chain or of an index has one level for each link.
    let long = [
        format!("local x = 1{}", " + 1".repeat(20_000)),
        format!("local y = obj{}", ":next(1)".repeat(20_000)),
        format!("local z = obj{}", ".field".repeat(20_000)),
        format!("{}local b = 1", "local a\n".repeat(20_000)),
    ];
    for text in &long {
        for style in [style(), every_option()] {
            assert_eq!(format(&Source::new(text), &style, None), Ok(Vec::new()), "written as it is formatted");
        }
    }
    // Each line of a sum is indented from the line the sum starts on, a level at a time.
    let lines = |count: usize| format!("local x = 1{}", "\n+ 1".repeat(count));
    let short = formatted(&Source::new(&lines(200)), &style());
    assert!(short.starts_with("local x = 1\n        + 1\n        + 1\n"), "{}", &short[..60]);
    assert_eq!(formatted(&Source::new(&short), &style()), short);
    let refused = format(&Source::new(&lines(8_000)), &style(), None);
    assert_eq!(refused.map(|edits| edits.len()), Err(Unsettled), "more than a layout may do");
}

#[test]
fn a_formatted_text_is_left_alone() {
    let corpus = [
        PREVIEW,
        include_str!("../lua/testdata/valid/cComment.lua"),
        include_str!("../lua/testdata/valid/compound.lua"),
        include_str!("../lua/testdata/valid/hash.lua"),
        include_str!("../lua/testdata/valid/inUnpack.lua"),
        include_str!("../lua/testdata/valid/lua54.lua"),
        include_str!("../lua/testdata/valid/safeNav.lua"),
        include_str!("../lua/testdata/valid/setConstructor.lua"),
        include_str!("../editor/testdata/structure.lua"),
        "local\nfor\nwhile do\nreturn return\nlocal function\nfunction(a, a",
        "a,b+=1\n(a+=1)\nx?\ni++\n++i\nz, x?.y = 1\nlocal a in {a=1}\nf() += 1\nt = { .end }",
        "Chain.first(a)\n    .second({\n        k = 1,\n    }).third(b)\nlocal x = Obj.method(a, function()\n    return 1\nend).other(b)",
        "local value = name.method(first,\n    second).other()\nBuilder.new(a, {\n    k = 1,\n}).add(x)",
    ];
    for text in corpus {
        let text = text.replace("\r\n", "\n");
        for style in [style(), Style::new(2, true), Style::new(4, false), every_option()] {
            let once = formatted(&Source::new(&text), &style);
            assert_eq!(formatted(&Source::new(&once), &style), once, "{text}");
            assert_eq!(code(&once, false), code(&text, false), "{text}");
            let lines = once.lines().all(|line| line == line.trim_end() || line.trim_start().starts_with("--"));
            assert!(lines, "white space at the end of a line of code: {once}");
        }
    }
}

#[test]
fn every_edit_replaces_text_of_the_file() {
    let text = "a=1;\r\nif a then\r\n  b ( 2 )\r\nend\r\n";
    let src = Source::new(text);
    let edits = format(&src, &style(), None).expect("a layout that settles");
    let mut last = 0;
    for (from, to, _) in &edits {
        assert!(last <= *from && from <= to && *to <= text.len() && text.is_char_boundary(*from), "{edits:?}");
        last = *to;
    }
    assert_eq!(
        formatted(&src, &style()),
        "a = 1\r\nif a then\r\n    b(2)\r\nend\r\n",
        "line breaks stay what they were"
    );
}

/// The style `FORMAT_STYLE` names: `options` turns on what is off by default, the simple blocks and the
/// alignments; `habits` is what a project written by hand often does, simple blocks on one line and one
/// indent for what continues a line.
fn asked() -> Style {
    match std::env::var("FORMAT_STYLE").as_deref() {
        Ok("habits") => Style { keep_simple_blocks: true, ..style() },
        Ok("options") => Style {
            keep_simple_blocks: true,
            align_arguments: true,
            align_consecutive_assignments: true,
            align_table_field_assign: true,
            space_inside_inline_table: false,
            ..style()
        },
        _ => style(),
    }
}

fn lua_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            lua_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "lua") {
            out.push(path);
        }
    }
}

/// Formats every Lua file under `CFXLUA_PROJECT` and checks what must hold of each one: its tokens stay
/// what they are, a second pass changes nothing, a file that parses still does. Prints numbers, and for a
/// file that fails its rank among the files and the name of the check: never a path, never text.
#[test]
#[ignore = "formats the project given in CFXLUA_PROJECT"]
fn format_cost_of_a_real_project() {
    let Ok(root) = std::env::var("CFXLUA_PROJECT") else { return };
    // A list of paths under the project, one a line, where the folder cannot be walked.
    let mut files = Vec::new();
    match std::env::var("CFXLUA_LIST") {
        Ok(list) => {
            let listed = std::fs::read_to_string(list).expect("the list");
            files.extend(listed.lines().map(|line| std::path::Path::new(&root).join(line)));
        }
        Err(_) => lua_files(std::path::Path::new(&root), &mut files),
    }
    files.sort();
    let options = asked();
    let single = std::env::var("FORMAT_STYLE").as_deref() == Ok("habits");
    let mut kinds: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let (mut tabbed, mut by_two, mut unsettled, mut given_up, mut ranged) = (0u32, 0u32, 0u32, 0u32, 0usize);
    let (mut read, mut bytes, mut changed, mut edits, mut unparsed, mut semicolons) =
        (0u32, 0usize, 0u32, 0usize, 0u32, 0usize);
    let (mut lines_before, mut lines_after) = (0usize, 0usize);
    let (mut total, mut slowest, mut slowest_size) = (std::time::Duration::ZERO, std::time::Duration::ZERO, 0usize);
    let mut broken: Vec<(usize, &str)> = Vec::new();
    for (rank, path) in files.iter().enumerate() {
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        if text.len() > 3_000_000 {
            continue;
        }
        let text = text.replace("\r\n", "\n");
        // The indent an editor detects: tabs where a line starts with one, else the smallest indent written.
        let indents = text.lines().map(|line| line.len() - line.trim_start_matches(' ').len());
        let unit = indents.filter(|indent| *indent > 0).min().unwrap_or(4).clamp(2, 4) as u32;
        let tabs = text.lines().any(|line| line.starts_with('\t'));
        let detected = Style::new(if tabs { 4 } else { unit }, !tabs);
        let style = Style {
            indent: detected.indent,
            continuation: if single { detected.indent } else { detected.continuation },
            tabs: detected.tabs,
            tab_size: detected.tab_size,
            ..options.clone()
        };
        tabbed += tabs as u32;
        by_two += (!tabs && unit < 4) as u32;
        let src = Source::new(&text);
        let errors = src.root().tree().has_errors();
        let started = std::time::Instant::now();
        let (mut found, settled) = laid_out(&src, &style, None);
        let mut refused = false;
        if !settled {
            unsettled += 1;
            found = format(&src, &style, None).unwrap_or_else(|_| {
                refused = true;
                Vec::new()
            });
            given_up += refused as u32;
        }
        let took = started.elapsed();
        read += 1;
        bytes += text.len();
        total += took;
        if took > slowest {
            (slowest, slowest_size) = (took, text.len());
        }
        unparsed += errors as u32;
        changed += !found.is_empty() as u32;
        edits += found.len();
        let ordered = found.windows(2).all(|pair| pair[0].1 <= pair[1].0);
        let inside = found.iter().all(|(from, to, _)| from <= to && *to <= text.len() && text.is_char_boundary(*from));
        if !ordered || !inside {
            broken.push((rank, "edits"));
            continue;
        }
        let tokens = src.root().tree().tokens();
        let named =
            |at: Option<usize>| at.and_then(|at| tokens.at(at)).map_or("none", |index| tokens.kind(index).name());
        for (from, to, with) in &found {
            let around = format!("{} | {}", named(from.checked_sub(1)), named((*to < text.len()).then_some(*to)));
            *kinds.entry(format!("{}: {around}", changed_as(&style, &text[*from..*to], with))).or_default() += 1;
        }
        let mut once = text.clone();
        for (from, to, with) in found.iter().rev() {
            once.replace_range(*from..*to, with);
        }
        let (before, after) = (code(&text, true), code(&once, true));
        semicolons += before.len() - after.len().min(before.len());
        lines_before += text.lines().count();
        lines_after += once.lines().count();
        if code(&text, false) != code(&once, false) {
            broken.push((rank, "tokens"));
        }
        let again = Source::new(&once);
        if again.root().tree().has_errors() != errors {
            broken.push((rank, "parse"));
        }
        if !refused && format(&again, &style, None) != Ok(Vec::new()) {
            broken.push((rank, "second pass"));
        }
        // A few lines at three places of the file: what Reformat Code does to a selection.
        let starts: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(at, _)| at + 1)).collect();
        for (place, lines) in [(1, 1), (2, 5), (3, 40)] {
            let first = starts.len() * place / 4;
            let last = (first + lines).min(starts.len() - 1);
            let (from, to) = (starts[first], starts[last]);
            let line_end = starts.get(last + 1).map_or(text.len(), |next| next - 1);
            let Ok(found) = format(&src, &style, Some((from, to))) else { continue };
            let ordered = found.windows(2).all(|pair| pair[0].1 <= pair[1].0);
            let touching = found.iter().all(|(start, end, _)| start <= end && *end >= from && *start <= line_end);
            if !ordered || !touching {
                broken.push((rank, "range edits"));
                continue;
            }
            ranged += found.len();
            let mut part = text.clone();
            for (start, end, with) in found.iter().rev() {
                part.replace_range(*start..*end, with);
            }
            if code(&text, false) != code(&part, false) {
                broken.push((rank, "range tokens"));
            }
            if Source::new(&part).root().tree().has_errors() != errors {
                broken.push((rank, "range parse"));
            }
        }
    }
    eprintln!(
        "{read} files, {} KB, {changed} changed, {} untouched, {edits} edits, {semicolons} semicolons out, {unparsed} with a syntax error",
        bytes / 1024,
        read - changed
    );
    eprintln!("{lines_before} lines before, {lines_after} after");
    eprintln!("{tabbed} files indented with tabs, {by_two} files by less than four spaces");
    eprintln!("{unsettled} files whose alignments did not settle, {given_up} of them left as they are");
    eprintln!("{ranged} edits of the three selections formatted in each file");
    eprintln!(
        "{} ms in all, {} ms for the slowest ({} KB)",
        total.as_millis(),
        slowest.as_millis(),
        slowest_size / 1024
    );
    let mut kinds: Vec<(String, usize)> = kinds.into_iter().collect();
    kinds.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let shown = std::env::var("FORMAT_KINDS").ok().and_then(|count| count.parse().ok()).unwrap_or(45);
    for (kind, count) in kinds.iter().take(shown) {
        eprintln!("{count} edits {kind}");
    }
    eprintln!("{} broken: {:?}", broken.len(), &broken[..broken.len().min(40)]);
    assert!(broken.is_empty());
}

/// For the files of `CFXLUA_LIST` whose ranks `FORMAT_RANKS` names: whether each of two passes settled, and
/// what the second one still changes, by kind. Numbers and kinds of tokens only.
#[test]
#[ignore = "explains why the files FORMAT_RANKS names change at a second pass"]
fn format_second_pass_of_some_files() {
    let (Ok(root), Ok(list), Ok(ranks)) =
        (std::env::var("CFXLUA_PROJECT"), std::env::var("CFXLUA_LIST"), std::env::var("FORMAT_RANKS"))
    else {
        return;
    };
    let listed = std::fs::read_to_string(list).expect("the list");
    let mut files: Vec<std::path::PathBuf> =
        listed.lines().map(|line| std::path::Path::new(&root).join(line)).collect();
    files.sort();
    let options = Style {
        keep_simple_blocks: true,
        align_arguments: std::env::var("FORMAT_NO_ARGUMENTS").is_err(),
        align_consecutive_assignments: std::env::var("FORMAT_NO_ASSIGNMENTS").is_err(),
        align_table_field_assign: std::env::var("FORMAT_NO_FIELDS").is_err(),
        space_inside_inline_table: false,
        ..style()
    };
    for rank in ranks.split(',').filter_map(|rank| rank.trim().parse::<usize>().ok()) {
        let text = std::fs::read_to_string(&files[rank]).expect("the file").replace("\r\n", "\n");
        let tabs = text.lines().any(|line| line.starts_with('\t'));
        let style = Style { tabs, ..options.clone() };
        let src = Source::new(&text);
        let (first, settled) = laid_out(&src, &style, None);
        let mut once = text.clone();
        for (from, to, with) in first.iter().rev() {
            once.replace_range(*from..*to, with);
        }
        let again = Source::new(&once);
        let (second, settled_again) = laid_out(&again, &style, None);
        let tokens = again.root().tree().tokens();
        let named =
            |at: Option<usize>| at.and_then(|at| tokens.at(at)).map_or("none", |index| tokens.kind(index).name());
        let mut kinds: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for (from, to, with) in &second {
            let around = format!("{} | {}", named(from.checked_sub(1)), named((*to < once.len()).then_some(*to)));
            *kinds.entry(format!("{}: {around}", changed_as(&style, &once[*from..*to], with))).or_default() += 1;
        }
        eprintln!(
            "{rank} file: {} KB, {} lines, settled {settled} then {settled_again}, {} edits then {}: {kinds:?}",
            text.len() / 1024,
            text.lines().count(),
            first.len(),
            second.len()
        );
        // Where the first edits of the second pass stand: the line, the columns, the kinds of the nodes around.
        let tree = again.root().tree();
        for (from, to, with) in second.iter().take(3) {
            let line = once[..*to].matches('\n').count() + 1;
            let old = once[*from..*to].rsplit('\n').next().unwrap_or_default().len();
            let new = with.rsplit('\n').next().unwrap_or_default().len();
            let around: Vec<&str> = match tokens.at(*to) {
                Some(index) => {
                    let node = tree.node(&once, tree.node_of_token(index));
                    std::iter::once(node).chain(node.ancestors()).map(|node| node.kind().name()).collect()
                }
                None => Vec::new(),
            };
            let width = once.lines().nth(line.saturating_sub(2)).map_or(0, str::len);
            eprintln!(
                "{rank} file: line {line}, {old} to {new} columns, the line before is {width} wide, in {around:?}"
            );
        }
    }
}

/// What an edit does to a white space, in a few words: the tokens around it are named by their kind, and
/// nothing of the text is shown.
fn changed_as(style: &Style, old: &str, new: &str) -> String {
    let breaks = |white: &str| white.matches('\n').count();
    let last = |white: &str| style.columns(white.rsplit('\n').next().unwrap_or(white).trim_end_matches('\r')) as i64;
    if old.contains(';') {
        return "semicolon".to_owned();
    }
    match (breaks(old), breaks(new)) {
        (0, 0) if last(old) < last(new) => "space added".to_owned(),
        (0, 0) => "space removed".to_owned(),
        (0, _) => "line break added".to_owned(),
        (_, 0) => "line break removed".to_owned(),
        (before, after) if before != after => "blank lines".to_owned(),
        _ if last(old) == last(new) && old.contains('\t') != new.contains('\t') => "tabs or spaces".to_owned(),
        _ if last(old) == last(new) => "white space at the end of a line".to_owned(),
        _ => format!("indent {:+}", last(new) - last(old)),
    }
}

#[test]
#[ignore = "prints what a file given in FORMAT_PROBE becomes"]
fn probe_a_file() {
    let Ok(path) = std::env::var("FORMAT_PROBE") else { return };
    let text = std::fs::read_to_string(path).expect("the file").replace("\r\n", "\n");
    if std::env::var("FORMAT_PARSE_ONLY").is_ok() {
        let src = Source::new(&text);
        eprintln!("{} nodes, errors: {}", src.root().tree().len(), src.root().tree().has_errors());
        return;
    }
    if std::env::var("FORMAT_COUNT").is_ok() {
        match format(&Source::new(&text), &asked(), None) {
            Ok(edits) => eprintln!("{} edits", edits.len()),
            Err(Unsettled) => eprintln!("not laid out"),
        }
        return;
    }
    let once = formatted(&Source::new(&text), &asked());
    eprintln!("{once}");
    let twice = formatted(&Source::new(&once), &asked());
    if twice != once {
        eprintln!("-- a second pass makes it:\n{twice}");
    }
}
