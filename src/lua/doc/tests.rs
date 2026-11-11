//! The parsing tests of the JetBrains plugin (`LuaCatsParsingTest`), on its own synthetic library.

use super::tags::{self, Comment, description, is_optional as optional, name, ty, value_names as names};
use crate::lua::Source;
use crate::lua::token::T;
use crate::lua::tree::{N, Node};

const LIBRARY: &str = include_str!("testdata/synthlib.lua");

/// The parser errors and the bad characters of the documentation comments, as `line: message`.
fn problems(src: &Source) -> Vec<String> {
    let root = src.docs();
    let line = |at: usize| src.text[..at].matches('\n').count() + 1;
    let tokens = root.tree().tokens();
    let errors = root.tree().errors(&src.text).map(|(node, message)| format!("{}: {message}", line(node.range().0)));
    let bad = (0..tokens.len()).filter(|i| tokens.kind(*i) == T::BadCharacter);
    errors.chain(bad.map(|i| format!("{}: bad character", line(tokens.start(i))))).collect()
}

/// The tokens of the documentation comments, white space left out, as `NAME text`.
fn lexed(text: &str) -> Vec<String> {
    let src = Source::new(text);
    let root = src.docs();
    root.all_tokens()
        .filter(|token| !token.kind().is_trivia())
        .map(|token| format!("{} {}", token.kind().name(), token.text()))
        .collect()
}

fn all<'a>(src: &'a Source, kind: N) -> Vec<Node<'a>> {
    src.docs().descendants().filter(|node| node.is(kind)).collect()
}

/// The documentation comment written right above `needle`.
fn above<'a>(src: &'a Source, needle: &str) -> Node<'a> {
    let at = src.text.find(needle).expect(needle);
    src.docs().children_of(N::DocComment).take_while(|comment| comment.range().1 <= at).last().expect("a comment above")
}

fn types(node: Node) -> Vec<Node> {
    tags::types(node).collect()
}

fn texts<'a>(nodes: Vec<Node<'a>>) -> Vec<&'a str> {
    nodes.into_iter().map(Node::code).collect()
}

fn param<'a>(comment: Node<'a>, name: &str) -> Node<'a> {
    Comment(comment).param(name).expect(name)
}

fn returned(comment: Node) -> Node {
    comment.child(N::DocTagReturn).and_then(|tag| tag.child(N::DocTypeList)).expect("a return tag")
}

#[test]
fn the_lexer_tells_a_type_from_its_description() {
    assert_eq!(
        lexed("---@param a number the first (optional) value\nlocal a"),
        ["DASHES ---", "@ @", "TAG_NAME_PARAM param", "ID a", "ID number", "STRING the first (optional) value"]
    );
    assert_eq!(
        lexed("---@return string? err Why it failed\nlocal a"),
        ["DASHES ---", "@ @", "TAG_NAME_RETURN return", "ID string", "? ?", "ID err", "STRING Why it failed"]
    );
    assert_eq!(
        lexed("---@type fun(x?: number, ...: string): boolean"),
        [
            "DASHES ---",
            "@ @",
            "TAG_NAME_TYPE type",
            "fun fun",
            "( (",
            "ID x",
            "? ?",
            ": :",
            "ID number",
            ", ,",
            "ID ...",
            ": :",
            "ID string",
            ") )",
            ": :",
            "ID boolean",
        ]
    );
    assert_eq!(
        lexed("---|>'warning' # The default type"),
        ["DASHES ---", "ALIAS_BAR |>", "STRING 'warning'", "STRING_BEGIN #", "STRING  The default type"]
    );
    assert_eq!(
        lexed("--- plain text\n---@module 'name'"),
        ["DASHES ---", "STRING plain text", "DASHES ---", "@ @", "TAG_NAME module", "STRING  'name'"]
    );
    assert_eq!(
        lexed("---@field private [string] T[] - text"),
        [
            "DASHES ---",
            "@ @",
            "TAG_NAME_FIELD field",
            "private private",
            "[ [",
            "ID string",
            "] ]",
            "ID T",
            "[] []",
            "STRING - text"
        ]
    );
}

#[test]
fn the_error_check_detects_doc_errors() {
    assert!(!problems(&Source::new("---@return ? @bad\nfunction f() end\n")).is_empty());
    assert!(!problems(&Source::new("---@param x number | \nfunction f(x) end\n")).is_empty());
}

#[test]
fn the_synthetic_library_parses_without_doc_errors() {
    assert_eq!(problems(&Source::new(LIBRARY)), Vec::<String>::new());
}

#[test]
fn optional_parameters_and_fields() {
    let src = Source::new(LIBRARY);
    let timeout = param(above(&src, "function lib.requestModel"), "timeout");
    assert!(optional(timeout));
    assert_eq!(ty(timeout).map(Node::code), Some("integer"));
    assert!(!optional(param(above(&src, "function lib.requestModel"), "model")));
    // `T?` and `T|nil` are optional too.
    assert!(optional(param(above(&src, "function lib.showTextUI"), "options")));

    let fields = all(&src, N::DocTagField);
    let field = |wanted: &str| *fields.iter().find(|field| name(**field) == Some(wanted)).expect(wanted);
    assert!(optional(field("title")));
    assert!(!optional(field("coords")));
    assert_eq!(ty(field("duration")).map(Node::code), Some("integer"));
    assert_eq!(description(field("duration")), "Milliseconds (default 3000)");
}

#[test]
fn index_signature_fields() {
    let src = Source::new(LIBRARY);
    let indexed: Vec<Node> = all(&src, N::DocTagField).into_iter().filter(|field| field.has(T::LBrack)).collect();
    assert_eq!(indexed.len(), 1);
    assert_eq!(name(indexed[0]), None);
    assert_eq!(texts(types(indexed[0])), ["string", "SynthPoint"]);
    let dictionary = all(&src, N::DocTableIndexField);
    assert_eq!(dictionary.len(), 1);
    assert_eq!(texts(types(dictionary[0])), ["string", "string | number"]);
}

#[test]
fn named_and_multiple_returns() {
    let src = Source::new(LIBRARY);
    let returns: Vec<Node> = above(&src, "function lib.checkDependency").children_of(N::DocTagReturn).collect();
    assert_eq!(returns.len(), 2);
    let lists: Vec<Node> = returns.iter().filter_map(|tag| tag.child(N::DocTypeList)).collect();
    assert_eq!(names(lists[0]), ["ok"]);
    assert_eq!(names(lists[1]), ["err"]);
    assert_eq!(description(returns[1]), "Why it failed");

    let count = returned(above(&src, "function lib.count"));
    assert_eq!(texts(types(count)), ["integer", "integer[]"]);
    assert_eq!(names(count), ["count", "handles"]);

    let sum = above(&src, "function lib.sumDiff");
    assert_eq!(names(returned(sum)), ["sum", "difference"]);
    assert_eq!(sum.child(N::DocTagReturn).map(description), Some("two values"));

    let level = returned(above(&src, "function lib.level"));
    assert_eq!(names(level), ["level", "visible"]);
    assert_eq!(level.descendants().filter(|node| node.is(N::DocNumberLiteralTy)).count(), 3);
}

#[test]
fn multi_line_aliases() {
    let src = Source::new(LIBRARY);
    let aliases = all(&src, N::DocTagAlias);
    let alias = *aliases.iter().find(|alias| name(**alias) == Some("SynthNotifyType")).expect("the alias");
    assert_eq!(ty(alias).map(Node::code), None);
    let members: Vec<Node> = alias.children_of(N::DocAliasMember).collect();
    assert_eq!(
        texts(members.iter().filter_map(|member| ty(*member)).collect()),
        ["'inform'", "'error'", "'success'", "'warning'"]
    );
    assert_eq!(members.iter().map(|member| tags::is_default(*member)).collect::<Vec<_>>(), [false, false, false, true]);
    assert_eq!(description(members[0]), "Informational (blue)");
    assert_eq!(param(above(&src, "function lib.count"), "kind").children_of(N::DocAliasMember).count(), 3);
}

#[test]
fn classes_enums_and_attributes() {
    let src = Source::new(LIBRARY);
    let classes = all(&src, N::DocTagClass);
    let class = |wanted: &str| *classes.iter().find(|class| name(**class) == Some(wanted)).expect(wanted);
    let is_enum = |class: Node| class.has(T::DocTagEnum);
    assert_eq!(texts(class("SynthLib").children_of(N::DocClassNameRef).collect()), ["SynthEvents", "SynthTimers"]);
    assert_eq!(class("SynthPoint").child(N::DocClassAttr).map(Node::code), Some("(exact)"));
    assert!(is_enum(class("SynthKeybindMode")));
    assert!(is_enum(class("SynthWeather")));
    assert!(!is_enum(class("SynthLib")));
}

#[test]
fn tags_of_lua_language_server_only_are_accepted() {
    let src = Source::new(LIBRARY);
    let tags = all(&src, N::DocTagDef);
    let found: Vec<&str> = tags.iter().filter_map(|tag| tag.token(T::DocTagName)).map(|tag| tag.text()).collect();
    for wanted in
        ["meta", "diagnostic", "async", "nodiscard", "package", "version", "source", "operator", "deprecated", "module"]
    {
        assert!(found.contains(&wanted), "{wanted} in {found:?}");
    }
    // `---@module 'name'` types a local as `require('name')`: no module is declared.
    assert!(all(&src, N::DocTagClass).iter().all(|class| !class.has(T::DocTagModule)));
    assert!(Comment(above(&src, "function lib.alert")).is_deprecated());
    assert!(!Comment(above(&src, "function lib.notify")).is_deprecated());
}

#[test]
fn function_types() {
    let src = Source::new(LIBRARY);
    let functions = all(&src, N::DocFunctionTy);
    let callback = *functions.iter().find(|function| function.code().starts_with("fun(x?")).expect("the callback");
    let params: Vec<Node> = callback.children_of(N::DocFunctionParam).collect();
    assert_eq!(params.len(), 2);
    assert!(optional(params[0]));
    assert_eq!(name(params[1]), Some("..."));
    assert_eq!(ty(params[1]).map(Node::code), Some("string"));
}

#[test]
fn casts() {
    let src = Source::new("local x\n---@cast x string\n---@cast x +nil, -number\n---@cast x -?");
    assert_eq!(problems(&src), Vec::<String>::new());
    let casts = all(&src, N::DocTagCast);
    assert_eq!(casts.len(), 3);
    assert_eq!(name(casts[0]), Some("x"));
    let signs = |cast: Node| {
        let items = cast.children_of(N::DocCastItem);
        items.map(|item| (item.has(T::Plus), item.has(T::Minus))).collect::<Vec<_>>()
    };
    assert_eq!(signs(casts[0]), [(false, false)]);
    assert_eq!(signs(casts[1]), [(true, false), (false, true)]);
    assert_eq!(signs(casts[2]), [(false, true)]);
}

#[test]
fn descriptions_after_a_complete_type_are_free_text() {
    let src = Source::new(
        "---@param a number the first (optional) value, or {nothing} [really]\n\
         ---@param b string 'quoted' description\n\
         ---@param c string # LuaCATS description\n\
         ---@param e function fun to call, vararg style\n\
         ---@return number the result\n\
         ---@field private d number - private field\n\
         function f(a, b, c, e) end",
    );
    assert_eq!(problems(&src), Vec::<String>::new());
    let comment = above(&src, "function f(");
    let typed = |wanted: &str| ty(param(comment, wanted)).map(Node::code);
    assert_eq!(typed("a"), Some("number"));
    assert_eq!(typed("b"), Some("string"));
    assert_eq!(typed("c"), Some("string"));
    assert_eq!(typed("e"), Some("function"));
    assert_eq!(texts(types(returned(comment))), ["number"]);
}

#[test]
fn emmydoc_snippets_keep_their_meaning() {
    let src = Source::new(
        "---@class Base\n---@field public name string @the name\n---@field protected id number\nlocal Base = {}\n\n\
         ---@class Derived : Base @a derived class\nlocal Derived = {}\n\n\
         ---@alias Handler fun(self:Derived, args:string[]):boolean\n---@alias Event '\"a\"' | '\"b\"'\n\n\
         ---@generic K, V : Base\n---@param t table<K, V>\n---@param cb fun(k:K, v:V):boolean, string\n\
         ---@param opt {x:number, y:number}|nil\n---@vararg number\n---@overload fun(t:table):void\n\
         ---@return number, string @two values\nfunction Derived:each(t, cb, opt, ...) end\n\n\
         ---@type Base[]\nlocal list = {}\n\n---@language JSON\nlocal json = \"{}\"\n\n\
         ---@see Base#name\n---@suppress unused\n---@private\nlocal hidden\n",
    );
    assert_eq!(problems(&src), Vec::<String>::new());
    let classes = all(&src, N::DocTagClass);
    let derived = *classes.iter().find(|class| name(**class) == Some("Derived")).expect("Derived");
    assert_eq!(texts(derived.children_of(N::DocClassNameRef).collect()), ["Base"]);
    let comment = above(&src, "function Derived:each");
    let params: Vec<Node> = comment.children_of(N::DocTagParam).collect();
    assert_eq!(params.len(), 3);
    let optionals: Vec<&str> = params
        .iter()
        .filter(|tag| optional(**tag))
        .filter_map(|tag| tag.child(N::DocParamNameRef))
        .map(Node::code)
        .collect();
    assert_eq!(optionals, ["opt"]);
    assert_eq!(texts(types(returned(comment))), ["number", "string"]);
    assert_eq!(names(returned(comment)), ["", ""]);
    assert_eq!(comment.child(N::DocTagReturn).map(description), Some("two values"));
    assert_eq!(all(&src, N::DocTagGenericList)[0].children_of(N::DocGenericDef).count(), 2);
    assert_eq!(all(&src, N::DocTagOverload).len(), 1);
    assert_eq!(all(&src, N::DocTagSuppress).len(), 1);
    assert_eq!(all(&src, N::DocAccessModifier).len(), 3);
}

#[test]
fn types_nest_as_their_operators_bind() {
    let shape = |text: &str| {
        let src = Source::new(text);
        let tag = all(&src, N::DocTagType)[0];
        let ty = ty(tag).expect("a type");
        let inner: Vec<String> =
            types(ty).into_iter().map(|part| format!("{} {}", part.kind().name(), part.code())).collect();
        format!("{} [{}]", ty.kind().name(), inner.join(", "))
    };
    assert_eq!(shape("---@type A|B[]|C?"), "UNION_TY [GENERAL_TY A, ARR_TY B[], NULLABLE_TY C?]");
    assert_eq!(shape("---@type table<string, A|B>[]"), "ARR_TY [GENERIC_TY table<string, A|B>]");
    assert_eq!(shape("---@type fun(a: A): B|C"), "FUNCTION_TY []");
    assert_eq!(shape("---@type (A|B)[]"), "ARR_TY [PAR_TY (A|B)]");
    assert_eq!(shape("---@type [string, integer]?"), "NULLABLE_TY [TUPLE_TY [string, integer]]");
}

#[test]
fn a_comment_being_typed_keeps_what_it_can() {
    let src = Source::new("---@param\n---@class\n---@field name\n---@type fun(a: \n---@alias\nlocal a");
    assert_eq!(all(&src, N::DocTagParam).len(), 1);
    assert_eq!(all(&src, N::DocTagClass).len(), 0);
    assert_eq!(all(&src, N::DocTagField).len(), 1);
    assert_eq!(all(&src, N::DocFunctionTy).len(), 1);
    assert_eq!(all(&src, N::DocTagAlias).len(), 1);
    assert!(!problems(&src).is_empty());
    let deep = format!("---@type {}A", "(".repeat(5_000));
    assert!(!problems(&Source::new(&deep)).is_empty());
}

#[test]
fn comments_are_parsed_apart() {
    let src = Source::new("---@param a string\nlocal x = 1\n---| 'b'\nlocal y\n  --- text\n  ---@type A\n");
    let comments: Vec<Node> = src.docs().children_of(N::DocComment).collect();
    assert_eq!(comments.len(), 3);
    assert_eq!(comments[0].children_of(N::DocTagParam).count(), 1);
    assert_eq!(all(&src, N::DocAliasMember)[0].parent(), Some(comments[1]));
    assert_eq!(comments[2].code(), "--- text\n  ---@type A");
    assert_eq!(src.docs().tree().node_at(&src.text, src.text.find("local y").unwrap_or(0)).kind(), N::DocFile);
    assert!(Source::new("local a = 1").docs().children().next().is_none());
    assert!(Source::plain("--- not Lua").docs().children().next().is_none());
}

/// The tokens of the documentation tree cover the text end to end, each one at least a byte long.
fn covered(src: &Source) -> bool {
    let root = src.docs();
    let tokens = root.tree().tokens();
    let ends = (0..tokens.len()).all(|i| tokens.start(i) < tokens.end(i));
    tokens.start(0) == 0 && tokens.end(tokens.len() - 1) == src.text.len() && (ends || src.text.is_empty())
}

#[test]
fn a_file_being_typed_is_read_at_every_keystroke() {
    for end in (0..=LIBRARY.len()).filter(|end| LIBRARY.is_char_boundary(*end)) {
        let src = Source::new(&LIBRARY[..end]);
        assert!(covered(&src), "the first {end} bytes");
    }
    // The same comments with what a keyboard leaves behind: unclosed quotes and brackets, stray signs.
    let comments = Source::new(LIBRARY).docs().children_of(N::DocComment).count();
    for junk in ["'", "\"", "(", "{", "[", "<", "|", "@", "`", "\\ ", "\t", "é", "---", "]]"] {
        let text = LIBRARY.replace("---@", &format!("---{junk}@")).replace(" string", &format!(" {junk}string"));
        let src = Source::new(&text);
        assert!(covered(&src), "with {junk:?}");
        assert_eq!(src.docs().children_of(N::DocComment).count(), comments, "with {junk:?}");
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

#[test]
#[ignore = "measurement: CFXLUA_PROJECT=<dir> cargo test --release --lib documentation_of_a_real_project -- --ignored --nocapture"]
fn documentation_of_a_real_project() {
    let root = std::path::PathBuf::from(std::env::var("CFXLUA_PROJECT").expect("CFXLUA_PROJECT"));
    let mut paths = Vec::new();
    lua_files(&root, &mut paths);
    let (mut files, mut documented, mut comments, mut tags, mut flagged) = (0, 0, 0, 0, 0);
    let mut by_message: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut by_tag: std::collections::BTreeMap<&'static str, usize> = std::collections::BTreeMap::new();
    let mut shapes: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut spent = std::time::Duration::ZERO;
    for path in &paths {
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        if text.len() > crate::workspace::MAX_FILE {
            continue;
        }
        files += 1;
        let src = Source::new(&text);
        let start = std::time::Instant::now();
        let root = src.docs();
        spent += start.elapsed();
        assert!(covered(&src));
        let found = root.children_of(N::DocComment).count();
        if found == 0 {
            continue;
        }
        documented += 1;
        comments += found;
        for node in root.descendants().filter(|node| node.kind().name().starts_with("TAG_")) {
            tags += 1;
            *by_tag.entry(node.kind().name()).or_default() += 1;
        }
        let problems = problems(&src);
        flagged += usize::from(!problems.is_empty());
        if std::env::var("CFXLUA_SHAPES").is_ok() {
            // The kinds of the tokens of each line with a problem, never their text.
            let tokens = root.tree().tokens();
            let line = |at: usize| src.text[..at].matches('\n').count() + 1;
            for problem in &problems {
                let wanted: usize = problem.split(':').next().and_then(|line| line.parse().ok()).unwrap_or(0);
                let kinds =
                    (0..tokens.len()).filter(|i| !tokens.kind(*i).is_trivia() && line(tokens.start(*i)) == wanted);
                let shape: Vec<&str> = kinds
                    .map(|i| if tokens.kind(i) == T::DocString { "TEXT" } else { tokens.kind(i).name() })
                    .collect();
                *shapes.entry(shape.join(" ")).or_default() += 1;
            }
        }
        for problem in problems {
            // Only the kind of problem is kept, never a piece of the file.
            let message = problem.split_once(": ").map_or("", |(_, message)| message).to_owned();
            *by_message.entry(message).or_default() += 1;
        }
    }
    eprintln!("{files} files, {documented} with documentation: {comments} comments, {tags} tags, read in {spent:?}");
    eprintln!("tags: {by_tag:?}");
    eprintln!("{flagged} files with documentation problems: {by_message:?}");
    let mut shapes: Vec<(usize, String)> = shapes.into_iter().map(|(shape, count)| (count, shape)).collect();
    shapes.sort_by(|a, b| b.cmp(a));
    for (count, shape) in shapes {
        eprintln!("  {count} x {shape}");
    }
}

#[test]
#[ignore = "the stubs of the JetBrains plugin: CFXLUA_INTELLIJ=<its repository> cargo test --lib the_stubs_of_the_jetbrains_plugin -- --ignored --nocapture"]
fn the_stubs_of_the_jetbrains_plugin_parse_without_doc_errors() {
    let repository = std::path::PathBuf::from(std::env::var("CFXLUA_INTELLIJ").expect("CFXLUA_INTELLIJ"));
    let mut stubs = Vec::new();
    lua_files(&repository.join("src/main/resources/std"), &mut stubs);
    stubs.sort();
    assert!(!stubs.is_empty());
    let (mut comments, mut tags) = (0, 0);
    for stub in &stubs {
        let text = std::fs::read_to_string(stub).expect("a stub");
        let src = Source::new(&text.replace("\r\n", "\n"));
        assert_eq!(problems(&src), Vec::<String>::new(), "{}", stub.display());
        comments += src.docs().children_of(N::DocComment).count();
        tags += src.docs().descendants().filter(|node| node.kind().name().starts_with("TAG_")).count();
    }
    eprintln!("{} stubs, {comments} documentation comments, {tags} tags", stubs.len());
}
