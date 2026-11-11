use super::parser::{INCREMENT_NOT_SUPPORTED, NESTED_TOO_DEEPLY};
use super::token::T;
use super::tree::{Element, N, Node, Tree, dump};

/// The snippets and trees of the IntelliJ plugin's parser tests (`src/test/resources/cfx/syntax/valid`).
const VALID: &[(&str, &str, &str)] = &[
    ("cComment", include_str!("testdata/valid/cComment.lua"), include_str!("testdata/valid/cComment.txt")),
    ("compound", include_str!("testdata/valid/compound.lua"), include_str!("testdata/valid/compound.txt")),
    ("hash", include_str!("testdata/valid/hash.lua"), include_str!("testdata/valid/hash.txt")),
    ("inUnpack", include_str!("testdata/valid/inUnpack.lua"), include_str!("testdata/valid/inUnpack.txt")),
    ("lua54", include_str!("testdata/valid/lua54.lua"), include_str!("testdata/valid/lua54.txt")),
    ("safeNav", include_str!("testdata/valid/safeNav.lua"), include_str!("testdata/valid/safeNav.txt")),
    (
        "setConstructor",
        include_str!("testdata/valid/setConstructor.lua"),
        include_str!("testdata/valid/setConstructor.txt"),
    ),
];

fn unix(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn errors(tree: &Tree, src: &str) -> Vec<String> {
    tree.errors(src).map(|(_, message)| message.to_owned()).collect()
}

fn statements<'a>(root: Node<'a>) -> Vec<Node<'a>> {
    root.children().filter(|node| node.kind().is_statement()).collect()
}

fn find<'a>(root: Node<'a>, kind: N) -> Node<'a> {
    root.descendants().find(|node| node.is(kind)).unwrap_or_else(|| panic!("no {kind:?} in {:?}", root.text()))
}

fn rebuilt(root: Node) -> String {
    fn walk(out: &mut String, node: Node) {
        for element in node.elements() {
            match element {
                Element::Token(token) => out.push_str(token.text()),
                Element::Node(child) => walk(out, child),
            }
        }
    }
    let mut out = String::new();
    walk(&mut out, root);
    out
}

/// `bad` reports `expected` (when given) and the `local ok = 1` that follows still parses as a statement.
fn invalid(bad: &str, expected: Option<&str>) {
    let src = format!("{bad}\nlocal ok = 1");
    let tree = Tree::parse(&src);
    let found = errors(&tree, &src);
    assert!(!found.is_empty(), "no error for {bad:?}");
    if let Some(expected) = expected {
        assert!(found.iter().any(|message| message == expected), "{bad:?}: expected {expected:?}, got {found:?}");
    }
    let root = tree.root(&src);
    let last = statements(root).pop().unwrap_or_else(|| panic!("{bad:?}: no statement"));
    assert_eq!(last.kind(), N::LocalDef, "{bad:?}");
    assert_eq!(last.text(), "local ok = 1", "{bad:?}");
    assert!(last.descendants().all(|node| !node.is(N::Error)), "{bad:?}");
    assert_eq!(rebuilt(root), src);
}

#[test]
fn valid_snippets_parse_to_the_intellij_trees() {
    for (name, lua, expected) in VALID {
        let src = unix(lua);
        let tree = Tree::parse(&src);
        assert_eq!(errors(&tree, &src), Vec::<String>::new(), "{name}");
        let (found, expected) = (dump(tree.root(&src)), unix(expected));
        if found.trim_end() != expected.trim_end() {
            let line = found.lines().zip(expected.lines()).position(|(a, b)| a != b).unwrap_or(0);
            let around = |text: &str| text.lines().skip(line.saturating_sub(4)).take(10).collect::<Vec<_>>().join("\n");
            panic!("{name}: line {}\n--- found\n{}\n--- expected\n{}", line + 1, around(&found), around(&expected));
        }
    }
}

#[test]
fn the_tree_keeps_every_character() {
    for (name, lua, _) in VALID {
        let src = unix(lua);
        assert_eq!(rebuilt(Tree::parse(&src).root(&src)), src, "{name}");
    }
}

#[test]
fn every_prefix_of_the_snippets_parses() {
    for (name, lua, _) in VALID {
        let src = unix(lua);
        for end in (0..=src.len()).filter(|end| src.is_char_boundary(*end)) {
            let prefix = &src[..end];
            let tree = Tree::parse(prefix);
            assert_eq!(rebuilt(tree.root(prefix)), prefix, "{name} cut at {end}");
        }
    }
}

#[test]
fn compound_assignment_shape() {
    let src = "t.a.b <<= 1 + 2";
    let tree = Tree::parse(src);
    let stat = find(tree.root(src), N::CompoundAssignStat);
    assert!(stat.has(T::BitLtLtAssign));
    let exprs: Vec<_> = stat.exprs().map(|e| (e.kind(), e.text())).collect();
    assert_eq!(exprs, [(N::IndexExpr, "t.a.b"), (N::BinaryExpr, "1 + 2")]);
}

#[test]
fn safe_navigation_shape() {
    let src = "local v = a?.b?[1]?:m()";
    let tree = Tree::parse(src);
    assert!(!tree.has_errors());
    let call = find(tree.root(src), N::CallExpr);
    assert_eq!(call.text(), "a?.b?[1]?:m()");
    let index: Vec<_> =
        call.descendants().filter(|n| n.is(N::IndexExpr)).map(|n| (n.text(), n.has(T::Question))).collect();
    assert_eq!(index, [("a?.b?[1]?:m", true), ("a?.b?[1]", true), ("a?.b", true)]);
}

#[test]
fn in_unpack_shape() {
    let src = "local a, b in t.x\nc, d.e in (f())";
    let tree = Tree::parse(src);
    assert!(!tree.has_errors());
    let root = tree.root(src);
    let local = find(root, N::LocalDef);
    assert!(local.has(T::In) && local.child(N::ExprList).is_none());
    assert_eq!(local.expr().map(Node::text), Some("t.x"));
    let assign = find(root, N::AssignStat);
    assert!(assign.has(T::In));
    assert_eq!(assign.expr().map(|e| (e.kind(), e.text())), Some((N::ParenExpr, "(f())")));
}

#[test]
fn set_field_shape() {
    let src = "t = { .a, .b = 1, [k], [\"s\"] = 2, c = 3, 4 }";
    let tree = Tree::parse(src);
    assert!(!tree.has_errors());
    let table = find(tree.root(src), N::TableExpr);
    let fields: Vec<_> = table.children_of(N::TableField).collect();
    assert_eq!(fields.len(), 6);
    assert_eq!(fields.iter().map(|f| f.has(T::Assign)).collect::<Vec<_>>(), [false, true, false, true, true, false]);
    assert!(fields[0].has(T::Dot));
    assert_eq!(fields[0].token(T::Id).map(|t| t.text()), Some("a"));
    assert_eq!(fields[2].expr().map(Node::text), Some("k"));
}

#[test]
fn hash_literal_shape() {
    let src = "local h = `adder`\nGetHashKey`adder`";
    let tree = Tree::parse(src);
    assert!(!tree.has_errors());
    let root = tree.root(src);
    assert!(find(root, N::LiteralExpr).has(T::HashString));
    let arg = find(find(root, N::CallExpr), N::SingleArg);
    assert!(arg.expr().is_some_and(|literal| literal.has(T::HashString)));
}

#[test]
fn invalid_code_reports_and_recovers() {
    let suffix = "expected suffixed expression after '?'";
    invalid("a,b+=1", Some("'=' expected"));
    invalid("(a+=1)", Some("')' expected"));
    invalid("x?", Some(suffix));
    invalid("local v = x??1", Some(suffix));
    invalid("local v = x ? 1 : 2", Some(suffix));
    invalid("i++", Some(INCREMENT_NOT_SUPPORTED));
    invalid("++i", Some(INCREMENT_NOT_SUPPORTED));
    invalid("local y = x++", Some(INCREMENT_NOT_SUPPORTED));
    invalid("z, x?.y = 1, 2", Some("'?' is only allowed in the first target of an assignment"));
    invalid("a:m?()", Some("'?' is not allowed between a method name and its arguments"));
    invalid("local a in {a=1}", Some("Name or '(' expected, the source of 'in' must be a suffixed expression"));
    invalid("local a in t, u", Some("Only one expression is allowed after 'in'"));
    invalid("a//=1", None);
    invalid("f() += 1", Some("Unexpected symbol '+='"));
    invalid("t = { .end }", Some("ID expected"));
    invalid("x = 1 ) ] }", Some("Unexpected symbol ')'"));
}

#[test]
fn errors_inside_a_function_body() {
    let src = "function f()\n    i++\n    x += 1\n    local v = a ?? b\n    y -= 2\nend\nlocal ok = 1";
    let tree = Tree::parse(src);
    assert!(errors(&tree, src).iter().any(|message| message == INCREMENT_NOT_SUPPORTED));
    let root = tree.root(src);
    let compounds: Vec<_> = root.descendants().filter(|n| n.is(N::CompoundAssignStat)).map(Node::text).collect();
    assert_eq!(compounds, ["x += 1", "y -= 2"]);
    let top = statements(root);
    assert_eq!(top.len(), 2);
    assert_eq!(top[0].kind(), N::FuncDef);
    assert_eq!(top[1].text(), "local ok = 1");
}

#[test]
fn compound_assignment_does_not_break_the_file() {
    let src = "x += 1\nlocal function f() return x end\nf()";
    let tree = Tree::parse(src);
    assert!(!tree.has_errors());
    assert_eq!(statements(tree.root(src)).len(), 3);
}

#[test]
fn documentation_comments_belong_to_the_statement_below() {
    let src = "--- about a\nlocal a = 1 --- behind a\n\n--- far above\n\nlocal b = 2\n-- plain\nlocal c = 3";
    let tree = Tree::parse(src);
    let top = statements(tree.root(src));
    assert_eq!(top[0].text(), "--- about a\nlocal a = 1 --- behind a");
    assert_eq!(top[0].code(), "local a = 1");
    assert_eq!(top[1].text(), "local b = 2");
    assert_eq!(top[2].text(), "local c = 3");
}

#[test]
fn a_block_holds_the_comments_around_its_statements() {
    let src = "do -- first\n  f()\n  -- last\nend";
    let tree = Tree::parse(src);
    let block = find(tree.root(src), N::Block);
    assert_eq!(block.text(), " -- first\n  f()\n  -- last\n");
}

#[test]
fn a_broken_statement_stays_inside_its_block() {
    let src = "if a then\n  x = = 2\nelse\n  y = 3\nend\nlocal ok = 1";
    let tree = Tree::parse(src);
    assert!(tree.has_errors());
    let top = statements(tree.root(src));
    assert_eq!(top.iter().map(|s| s.kind()).collect::<Vec<_>>(), [N::IfStat, N::LocalDef]);
    assert_eq!(top[0].children_of(N::Block).count(), 2);
}

fn shape(node: Node) -> String {
    let operator = |kind: N| node.child(kind).map_or("", Node::code);
    let operands: Vec<String> = node.exprs().map(shape).collect();
    match node.kind() {
        N::BinaryExpr => format!("({} {} {})", operands[0], operator(N::BinaryOp), operands.get(1).map_or("?", |s| s)),
        N::UnaryExpr => format!("({} {})", operator(N::UnaryOp), operands.first().map_or("?", |s| s)),
        _ => node.code().to_owned(),
    }
}

fn value_shape(src: &str) -> String {
    let tree = Tree::parse(src);
    let list = find(tree.root(src), N::ExprList);
    shape(list.expr().expect("a value"))
}

#[test]
fn operators_bind_like_lua() {
    assert_eq!(
        value_shape("x = a or b and c < d | e ~ f & g << h .. i + j * k ^ -l"),
        "(a or (b and (c < (d | (e ~ (f & (g << (h .. (i + (j * (k ^ (- l))))))))))))"
    );
    assert_eq!(value_shape("x = 1 - 2 - 3"), "((1 - 2) - 3)");
    assert_eq!(value_shape("x = a .. b .. c"), "(a .. (b .. c))");
    assert_eq!(value_shape("x = 2 ^ 3 ^ 2"), "(2 ^ (3 ^ 2))");
    assert_eq!(value_shape("x = -y ^ 2"), "(- (y ^ 2))");
    assert_eq!(value_shape("x = not a == b"), "((not a) == b)");
    assert_eq!(value_shape("x = a * b + c * d"), "((a * b) + (c * d))");
    assert_eq!(value_shape("x = a + b * c + d"), "((a + (b * c)) + d)");
    assert_eq!(value_shape("x = #t + 1"), "((# t) + 1)");
}

#[test]
fn a_missing_operand_keeps_the_looser_operators() {
    let src = "x = a * or b";
    let tree = Tree::parse(src);
    assert_eq!(errors(&tree, src), ["Expression expected"]);
    assert_eq!(value_shape(src), "((a * ?) or b)");
}

#[test]
fn a_long_chain_of_operators_parses() {
    let src = format!("x = 1{}", " + 1".repeat(20_000));
    let tree = Tree::parse(&src);
    assert!(!tree.has_errors());
    assert_eq!(tree.root(&src).descendants().filter(|node| node.is(N::BinaryExpr)).count(), 20_000);
    let src = format!("x = a{}", " .. a".repeat(150));
    assert!(!Tree::parse(&src).has_errors());
}

#[test]
fn deep_nesting_is_reported_not_overflowed() {
    let run = |src: String| {
        std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(move || {
                let tree = Tree::parse(&src);
                (errors(&tree, &src), rebuilt(tree.root(&src)) == src)
            })
            .expect("thread")
            .join()
            .expect("the parser overflowed its stack")
    };
    let cases = [
        format!("x = {}1{}", "(".repeat(5000), ")".repeat(5000)),
        format!("x = {}1{}", "{".repeat(5000), "}".repeat(5000)),
        format!("x = {}1", "- ".repeat(5000)),
        format!("{}x = 1{}", "do ".repeat(5000), " end".repeat(5000)),
        format!("{}x = 1", "if a then ".repeat(5000)),
        format!("x = {}1{}", "f(".repeat(5000), ")".repeat(5000)),
        format!("x = {}1{}", "function() return ".repeat(5000), " end".repeat(5000)),
        format!("x = a{}", " .. a".repeat(5000)),
        format!("x = a{}", " ^ a".repeat(5000)),
    ];
    for src in cases {
        let head = src[..20].to_owned();
        let (found, same) = run(src);
        assert!(same, "{head}");
        assert!(found.iter().any(|message| message == NESTED_TOO_DEEPLY), "{head}: {found:?}");
    }
}

#[test]
fn a_function_left_open_between_brackets_ends_at_the_bracket() {
    let src = "AddEventHandler('x', function(a)\n  local b = a\n)\nprint(1)";
    let tree = Tree::parse(src);
    assert_eq!(errors(&tree, src), ["'end' expected"]);
    let top = statements(tree.root(src));
    assert_eq!(top.iter().map(|s| s.code()).collect::<Vec<_>>(), [&src[..src.len() - 9], "print(1)"]);
    let src = "function f()\n  on('x', function()\n    if a then\n      g({ h(function() i() })\n  )\n  j()\nend\nk()";
    let tree = Tree::parse(src);
    let top = statements(tree.root(src));
    assert_eq!(top.len(), 2, "{:?}", errors(&tree, src));
    assert_eq!(top[1].code(), "k()");
    let body = find(top[0], N::Block);
    assert_eq!(statements(body).last().map(|s| s.code()), Some("j()"));
}

#[test]
fn shebang_line() {
    let src = "#!/usr/bin/env lua\nprint(1)";
    let tree = Tree::parse(src);
    assert!(!tree.has_errors());
    let root = tree.root(src);
    assert_eq!(root.child(N::ShebangLine).map(Node::text), Some("#!/usr/bin/env lua"));
    assert_eq!(statements(root).len(), 1);
}

#[test]
fn node_lookup_by_offset() {
    let src = "local a = f(1, g(2))\nreturn a";
    let tree = Tree::parse(src);
    let at = |needle: &str| tree.node_at(src, src.find(needle).expect("needle"));
    assert_eq!(at("2").kind(), N::LiteralExpr);
    assert_eq!(at("g").kind(), N::NameExpr);
    assert_eq!(at("g").parent().map(Node::text), Some("g(2)"));
    assert_eq!(at("return").kind(), N::ReturnStat);
    assert_eq!(tree.node_at(src, src.len()).kind(), N::NameExpr);
}

#[test]
#[ignore = "measurement: CFXLUA_PROJECT=<dir> cargo test --release --lib parse_cost_of_a_real_project -- --ignored --nocapture"]
fn parse_cost_of_a_real_project() {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name == "node_modules" {
                continue;
            }
            if path.is_dir() {
                walk(&path, out);
            } else if name.ends_with(".lua") {
                out.push(path);
            }
        }
    }
    let root = std::path::PathBuf::from(std::env::var("CFXLUA_PROJECT").expect("CFXLUA_PROJECT"));
    let mut paths = Vec::new();
    walk(&root, &mut paths);
    let files: Vec<(String, String)> = paths
        .iter()
        .filter_map(|path| {
            let relative = crate::text::slash(&path.strip_prefix(&root).ok()?.to_string_lossy());
            Some((relative, std::fs::read_to_string(path).ok()?))
        })
        .filter(|(_, text)| text.len() <= crate::workspace::MAX_FILE && !crate::project::is_escrowed(text))
        .collect();
    let bytes: usize = files.iter().map(|(_, text)| text.len()).sum();
    let start = std::time::Instant::now();
    let lexed: usize = files.iter().map(|(_, text)| super::token::lex(text).len()).sum();
    let lex_time = start.elapsed();
    let start = std::time::Instant::now();
    let trees: Vec<Tree> = files.iter().map(|(_, text)| Tree::parse(text)).collect();
    let parse_time = start.elapsed();
    let nodes: usize = trees.iter().map(Tree::len).sum();
    let (mut broken, mut shown, mut largest) = (0, 0, (0usize, 0usize, 0usize));
    for ((path, text), tree) in files.iter().zip(&trees) {
        assert_eq!(rebuilt(tree.root(text)), *text, "{path}");
        largest = largest.max((text.len(), tree.tokens().len(), tree.len()));
        if !tree.has_errors() {
            continue;
        }
        broken += 1;
        if shown < 25 {
            shown += 1;
            let (node, message) = tree.errors(text).next().expect("an error");
            let line = text[..node.range().0].matches('\n').count() + 1;
            eprintln!("  {path}:{line}: {message} ({} errors)", tree.errors(text).count());
        }
    }
    eprintln!("{} files, {bytes} bytes, {lexed} tokens, {nodes} nodes", files.len());
    eprintln!("lex {lex_time:?}, lex + parse {parse_time:?}, {broken} files with errors");
    eprintln!("largest file: {} bytes, {} tokens, {} nodes", largest.0, largest.1, largest.2);
}
