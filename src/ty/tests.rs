use super::doc::Reader;
use super::*;
use crate::lua::Source;
use crate::lua::doc::tags::{self, Comment};
use crate::lua::tree::{N, Node};

const LIBRARY: &str = include_str!("../lua/doc/testdata/synthlib.lua");

const READER: Reader = Reader { file: 7, bounded_generics: false };

fn comments(src: &Source) -> Vec<Comment<'_>> {
    src.docs().children_of(N::DocComment).map(Comment).collect()
}

/// The documentation comment written right above `needle`.
fn above<'a>(src: &'a Source, needle: &str) -> Comment<'a> {
    let at = src.text.find(needle).expect(needle);
    comments(src).into_iter().take_while(|comment| comment.0.range().1 <= at).last().expect("a comment above")
}

/// The type `---@type <text>` gives.
fn typed(text: &str) -> Ty {
    let src = Source::new(&format!("---@type {text}\nlocal value"));
    READER.typed(comments(&src)[0]).expect("a type")
}

fn shown(text: &str) -> String {
    display(&typed(text))
}

fn function(ty: &Ty) -> &Function {
    let Ty::Function(function) = ty else { panic!("{ty:?} is no function") };
    function
}

fn names(signature: &Signature) -> Vec<String> {
    signature.params.iter().map(Param::display_name).collect()
}

fn known(ret: &Ret) -> Ty {
    match ret {
        Ret::Ty(ty) => ty.clone(),
        Ret::Body(_) => Ty::Unknown,
    }
}

#[test]
fn a_union_keeps_each_type_once_and_drops_what_says_nothing() {
    assert_eq!(Ty::Unknown.union(&Ty::NUMBER), Ty::NUMBER);
    assert_eq!(Ty::NUMBER.union(&Ty::Nil), Ty::NUMBER);
    assert_eq!(Ty::NUMBER.union(&Ty::Void), Ty::NUMBER);
    assert_eq!(Ty::Unknown.union(&Ty::Nil), Ty::Nil);
    assert_eq!(Ty::Nil.union(&Ty::Unknown), Ty::Unknown);

    let both = Ty::NUMBER.union(&Ty::STRING);
    assert_eq!(display(&both), "number|string");
    assert_eq!(both.union(&Ty::NUMBER), both);
    assert_eq!(Ty::NUMBER.union(&Ty::NUMBER), Ty::NUMBER);
    assert_eq!(display(&both.union(&Ty::BOOLEAN)), "number|string|boolean");
    assert_eq!(display(&Ty::BOOLEAN.union(&both)), "number|string|boolean", "a union comes first");
    assert_eq!(display(&both.union(&Ty::BOOLEAN.union(&Ty::STRING))), "number|string|boolean");
    assert_eq!(both.each().len(), 2);
    assert_eq!(Ty::NUMBER.each(), [Ty::NUMBER]);

    let mut gathered = Ty::Unknown;
    for ty in [Ty::Nil, Ty::STRING, Ty::Void, Ty::STRING] {
        gathered.add(&ty);
    }
    assert_eq!(gathered, Ty::STRING);
}

#[test]
fn a_class_is_its_name_and_its_flags() {
    let class = Ty::class("Foo");
    let global = Ty::flagged("Foo", "Foo", GLOBAL);
    assert_eq!(class, Ty::class("Foo"));
    assert_ne!(class, global);
    assert_ne!(class, Ty::class("Bar"));
    assert_eq!(class.union(&global).each().len(), 2);
    assert_eq!(class, Ty::param("Foo", Some("Base".into())), "a generic parameter is the class of its name");
    assert_eq!(Ty::named("integer"), Ty::NUMBER);
    assert_eq!(Ty::named("unknown"), Ty::Unknown);
    assert_eq!(Ty::named("Foo"), class);
    assert_eq!(class.class_name(), Some("Foo"));
    assert_eq!(Ty::STRING.class_name(), Some("string"), "the members of a string are those of the library");
    assert_eq!(Ty::NUMBER.class_name(), None);
    assert_eq!(global_type_name("Foo"), "$Foo");
    assert_eq!(global_type_name("_G"), "_G");
}

#[test]
fn the_class_that_says_the_most_is_picked() {
    let local = Ty::flagged("12@7", "player", ANONYMOUS);
    let global = Ty::flagged("$Player", "Player", GLOBAL);
    let class = Ty::class("Player");
    let all = local.union(&global).union(&Ty::NUMBER).union(&class).union(&Ty::class("Other"));
    assert_eq!(all.perfect_class(), Some(&class));
    assert_eq!(local.union(&global).perfect_class(), Some(&global));
    assert_eq!(local.union(&Ty::NUMBER).perfect_class(), Some(&local));
    assert_eq!(Ty::NUMBER.perfect_class(), None);
    assert_eq!(all.top_classes(), [&local, &global, &class, &Ty::class("Other")]);
    assert_eq!(Ty::tuple(vec![class.clone(), global.clone()]).top_classes(), [&class], "the first value of a tuple");
    assert!(Ty::tuple(Vec::new()).top_classes().is_empty());
}

#[test]
fn types_are_shown_as_the_jetbrains_plugin_shows_them() {
    assert_eq!(display(&Ty::Unknown), "any");
    assert_eq!(display(&Ty::Void), "void");
    assert_eq!(display(&Ty::Nil), "nil");
    assert_eq!(display(&Ty::flagged("12@7", "player", ANONYMOUS)), "[local player]");
    assert_eq!(display(&Ty::flagged("$Player", "Player", GLOBAL)), "[global Player]");
    assert_eq!(display(&Ty::flagged("7@(30)table", "", ANONYMOUS | ANONYMOUS_TABLE)), "table");
    assert_eq!(display(&Ty::tuple(vec![Ty::NUMBER])), "number");
    assert_eq!(display(&Ty::tuple(vec![Ty::NUMBER, Ty::STRING])), "(number, string)");
    assert_eq!(display(&Ty::StringLiteral("a".into())), "\"a\"");
    assert_eq!(display(&Ty::StringLiteral("\"a\"".into())), "\"a\"", "the older notation holds its quotes");
    assert_eq!(literal_value("\"a\""), "a");
    assert_eq!(literal_value("'a'"), "a");
    assert_eq!(literal_value("a"), "a");
    assert_eq!(literal_value("\""), "\"");

    let lazy = Signature { ret: Ret::Body(FuncRef { file: 7, at: 40 }), ..Signature::new(false, Vec::new(), Ty::Void) };
    assert_eq!(display(&Ty::function(Function::new(lazy))), "fun(): any", "a body is not read to show a type");
}

#[test]
fn the_types_of_a_comment() {
    assert_eq!(shown("string"), "string");
    assert_eq!(shown("integer"), "number");
    assert_eq!(shown("any"), "any");
    assert_eq!(shown("Foo"), "Foo");
    assert_eq!(shown("`T`"), "T");
    assert_eq!(shown("string[]"), "string[]");
    assert_eq!(shown("string[][]"), "string[][]");
    assert_eq!(shown("table<string, Foo[]>"), "table<string, Foo[]>");
    assert_eq!(shown("string | number"), "string|number");
    assert_eq!(shown("(string | number)[]"), "string|number[]");
    assert_eq!(shown("string?"), "string", "nil is never kept in a union");
    assert_eq!(shown("string | nil"), "string");
    assert_eq!(typed("any?"), Ty::Nil);
    assert_eq!(shown("'a' | 'b' | \"c\""), "\"a\"|\"b\"|\"c\"");
    assert_eq!(shown("'\"old\"'"), "\"old\"");
    assert_eq!(shown("1 | 2 | 3"), "number");
    assert_eq!(shown("true | false"), "boolean");
    assert_eq!(shown("[string, number]"), "string|number[]", "a tuple is an array of its members");
    assert_eq!(shown("{ [string]: number }"), "table<string, number>");
    assert_eq!(shown("{ x: number, y?: string }"), "{ x: number, y: string }");
    assert_eq!(shown("fun()"), "fun(): void");
    assert_eq!(shown("fun(a: number, b?: string): boolean"), "fun(a: number, b?: string): boolean");
    assert_eq!(shown("fun(a, b: Foo[]): Foo, string"), "fun(a: any, b: Foo[]): (Foo, string)");
    assert_eq!(shown("fun(cb: fun(value: T): boolean): T[]"), "fun(cb: fun(value: T): boolean): T[]");
}

#[test]
fn a_table_of_a_comment_is_a_class_of_its_own() {
    let table = typed("{ x: number, y?: string }");
    let Ty::Class(class) = &table else { panic!("{table:?} is no class") };
    assert_eq!(&*class.name, "10|7|9", "the file and where the table is written");
    let Detail::Table(fields) = &class.detail else { panic!("no fields") };
    let read: Vec<(&str, String, bool)> =
        fields.iter().map(|field| (&*field.name, display(&field.ty), field.optional)).collect();
    assert_eq!(read, [("x", "number".to_owned(), false), ("y", "string".to_owned(), true)]);
    assert_ne!(typed("{ x: number }"), typed("---\n{ x: number }"), "two tables are two classes");
    assert!(matches!(typed("{ [string]: number, [number]: string }"), Ty::Class(_)), "two index signatures");
    assert!(matches!(typed("{ [string]: number, name: string }"), Ty::Class(_)));
}

#[test]
fn a_function_type_tells_its_parameters_from_its_extra_arguments() {
    let callback = typed("fun(x?: number, ...: string): boolean");
    let signature = &function(&callback).main;
    assert_eq!(names(signature), ["x?"]);
    assert_eq!(signature.vararg.as_ref().map(display).as_deref(), Some("string"));
    assert_eq!(display(&known(&signature.ret)), "boolean");
    assert!(!signature.colon_call && !signature.is_generic());

    let old = typed("fun(a: number, vararg string)");
    assert_eq!(names(&function(&old).main), ["a"]);
    assert_eq!(function(&old).main.vararg, Some(Ty::STRING));
    assert_eq!(function(&typed("fun(a: number)")).main.vararg, None);
    assert_eq!(function(&typed("fun(...)")).main.vararg, Some(Ty::Unknown));

    let generic = typed("fun<T, V : Base>(list: T[], value?: V): T");
    let generics = &function(&generic).main.generics;
    assert_eq!(generics.len(), 2);
    assert_eq!((&*generics[0].name, generics[0].base.as_deref()), ("T", None));
    assert_eq!((&*generics[1].name, generics[1].base.as_deref()), ("V", Some("Base")));
    assert!(function(&generic).main.params[1].optional);
    assert!(function(&typed("fun(a: string | nil)")).main.params[0].optional, "a type that admits nil");
}

#[test]
fn functions_are_the_same_when_they_take_the_same_types() {
    assert_eq!(typed("fun(a: number): string"), typed("fun(b: number): boolean"));
    assert_ne!(typed("fun(a: number)"), typed("fun(a: string)"));
    assert_ne!(typed("fun(a: number)"), typed("fun(a: number, b: number)"));
    assert_eq!(typed("fun(a: number): string | fun(b: number): boolean").each().len(), 1);
}

#[test]
fn the_tags_of_the_synthetic_library() {
    let src = Source::new(LIBRARY);
    let param = |function: &str, name: &str| {
        let comment = above(&src, function);
        display(&READER.param(comment.param(name).expect(name), comment))
    };
    assert_eq!(param("function lib.requestModel", "model"), "string|number");
    assert_eq!(param("function lib.requestModel", "timeout"), "number");
    assert_eq!(param("function lib.showTextUI", "options"), "table");
    assert_eq!(param("function lib.count", "kind"), "\"ped\"|\"vehicle\"|\"object\"", "only `---|` lines");
    assert_eq!(param("function lib.find", "list"), "T[]");
    assert_eq!(param("function lib.find", "fn"), "fun(value: T, index: number): boolean");

    let returns = |function: &str| READER.returns(above(&src, function)).map(|ty| display(&ty));
    assert_eq!(returns("function lib.requestModel").as_deref(), Some("number"));
    assert_eq!(returns("function lib.checkDependency").as_deref(), Some("(boolean, string)"), "two tags");
    assert_eq!(returns("function lib.count").as_deref(), Some("(number, number[])"));
    assert_eq!(returns("function lib.find").as_deref(), Some("T"));
    assert_eq!(returns("function lib.level").as_deref(), Some("(number, boolean)"));
    assert_eq!(returns("function lib.notify"), None);

    let join = above(&src, "function lib.join");
    assert_eq!(READER.vararg(join), Some(Ty::STRING), "`@param ... T` types the extra arguments");
    assert_eq!(READER.vararg(above(&src, "function lib.notify")), None);

    let alias = |wanted: &str| {
        let aliases = comments(&src).into_iter().flat_map(|comment| comment.tags(N::DocTagAlias));
        let tag = aliases.into_iter().find(|tag| tags::name(*tag) == Some(wanted)).expect(wanted);
        display(&READER.alias(tag))
    };
    assert_eq!(alias("SynthNotifyType"), "\"inform\"|\"error\"|\"success\"|\"warning\"");
    assert_eq!(alias("SynthId"), "number|string");
    assert!(alias("SynthPosition").starts_with("\"top\"|\"top-right\""));

    let field = |wanted: &str| {
        let fields = comments(&src).into_iter().flat_map(|comment| comment.tags(N::DocTagField));
        let tag: Node = fields.into_iter().find(|tag| tags::name(*tag) == Some(wanted)).expect(wanted);
        display(&READER.tag(tag))
    };
    assert_eq!(field("icon"), "string|string[]");
    assert_eq!(field("style"), "table<string, string|number>");
    assert_eq!(field("onEnter"), "fun(self: SynthPoint): void");
    assert_eq!(field("setTimeout"), "fun(ms: number, cb: function): number");
    assert_eq!(field("coords"), "vector3");

    let declared = |needle: &str| READER.declared(above(&src, needle)).map(|ty| display(&ty));
    assert_eq!(declared("lib = {}").as_deref(), Some("SynthLib"), "the class a comment declares");
    assert_eq!(declared("lib.points = {}").as_deref(), Some("table<string, SynthPoint>"));
    assert_eq!(declared("lib.pointList = {}").as_deref(), Some("SynthPoint[]"));
    assert_eq!(declared("lib.store = {}").as_deref(), Some("SynthStore"));
    assert_eq!(declared("function lib.notify"), None);
    assert_eq!(READER.typed(above(&src, "lib = {}")), None);
}

#[test]
fn several_return_tags_give_one_tuple() {
    let src = Source::new("---@return number, string\n---@return boolean\n---@return\nfunction f() end");
    let returned = READER.returns(comments(&src)[0]).expect("a return");
    assert_eq!(display(&returned), "(number, string, boolean)");
    let none = Source::new("---@return\nfunction f() end");
    assert_eq!(READER.returns(comments(&none)[0]), Some(Ty::Void));
}

#[test]
fn overloads_and_extra_arguments_of_a_comment() {
    let src = Source::new(
        "---@overload fun(a: number): number\n---@overload fun(a: string, b: string): string\n---@vararg number\n\
         function Shop:total(...) end",
    );
    let comment = comments(&src)[0];
    let overloads = READER.overloads(comment, true);
    assert_eq!(overloads.len(), 2);
    assert!(overloads.iter().all(|signature| signature.colon_call));
    assert_eq!(names(&overloads[1]), ["a", "b"]);
    assert_eq!(READER.vararg(comment), Some(Ty::NUMBER));
}

#[test]
fn a_bounded_generic_gives_the_members_of_its_bound_when_asked() {
    let src = Source::new("---@generic T : Base, V\n---@param a T\n---@param b V[]\nfunction f(a, b) end");
    let comment = comments(&src)[0];
    let plain = READER.param(comment.param("a").expect("a"), comment);
    assert!(matches!(&plain, Ty::Class(class) if matches!(class.detail, Detail::Named)));
    let bounded = Reader { bounded_generics: true, ..READER };
    let ty = bounded.param(comment.param("a").expect("a"), comment);
    assert!(matches!(&ty, Ty::Class(class) if matches!(&class.detail, Detail::Param(Some(base)) if &**base == "Base")));
    assert_eq!(ty, plain, "still the class T");
    assert_eq!(display(&bounded.param(comment.param("b").expect("b"), comment)), "V[]", "no bound, no change");
}

#[test]
fn a_call_reads_its_generic_parameters_from_its_arguments() {
    let ret = |ret: &Ret| known(ret);
    let bound = |declared: &str, given: Ty| {
        let mut bindings: Bindings = vec![("T".into(), Ty::Unknown), ("K".into(), Ty::Unknown)];
        analyze(&given, &typed(declared), &mut bindings, &ret);
        format!("T={} K={}", display(&bindings[0].1), display(&bindings[1].1))
    };
    let emmy = Ty::class("Emmy");
    assert_eq!(bound("T", emmy.clone()), "T=Emmy K=any");
    assert_eq!(bound("T[]", Ty::array(emmy.clone())), "T=Emmy K=any");
    assert_eq!(bound("T[]", emmy.clone()), "T=any K=any", "no array is given");
    assert_eq!(bound("T[]", Ty::array(emmy.clone()).union(&Ty::array(Ty::NUMBER))), "T=Emmy|number K=any");
    assert_eq!(bound("table<K, T>", typed("table<string, Emmy>")), "T=Emmy K=string");
    assert_eq!(bound("fun(): T", typed("fun(a: number): Emmy")), "T=Emmy K=any");
    assert_eq!(bound("T | K[]", Ty::array(Ty::NUMBER)), "T=number[] K=number");
    assert_eq!(bound("Other", emmy.clone()), "T=any K=any");
    assert_eq!(bound("T", Ty::Nil), "T=any K=any", "nothing is read from a type that says nothing");
}

#[test]
fn classes_are_rewritten_through_a_type() {
    let ty = typed("fun(a: self[], b: table<string, self>): self | Other, self");
    let rewritten = ty.substitute(&mut |class| (&*class.name == SELF).then(|| Ty::class("Player")));
    assert_eq!(
        display(&rewritten),
        "fun(a: Player[], b: table<string, Player>): (Player|Other, Player)",
        "{rewritten:?}"
    );
    let gone = typed("T | number").substitute(&mut |class| (&*class.name == "T").then_some(Ty::Unknown));
    assert_eq!(gone, Ty::NUMBER);
    let lazy = Signature { ret: Ret::Body(FuncRef { file: 7, at: 3 }), ..Signature::new(false, Vec::new(), Ty::Void) };
    let kept = Ty::function(Function::new(lazy)).substitute(&mut |_| Some(Ty::NUMBER));
    assert!(matches!(function(&kept).main.ret, Ret::Body(FuncRef { file: 7, at: 3 })));
}

#[test]
fn a_call_counts_the_receiver_of_a_method() {
    let params = vec![Param::new("a", Ty::NUMBER), Param::new("b", Ty::STRING)];
    let method = Signature::new(true, params.clone(), Ty::Void);
    let function = Signature::new(false, params, Ty::Void);
    let player = Ty::class("Player");
    let listed = |params: Vec<Param>| params.iter().map(|param| param.name.to_string()).collect::<Vec<_>>().join(", ");
    assert_eq!(listed(method.args(Some(&player), true)), "a, b");
    assert_eq!(listed(method.args(Some(&player), false)), "self, a, b", "a method called with a dot");
    assert_eq!(method.args(Some(&player), false)[0].ty, player);
    assert_eq!(listed(function.args(None, false)), "a, b");
    assert_eq!(listed(function.args(None, true)), "b", "a function called with a colon");
    assert_eq!(listed(method.declared(None)), "self, a, b");
    assert_eq!(listed(function.declared(None)), "a, b");
    assert_eq!(function.param_ty(1), Ty::STRING);
    assert_eq!(function.param_ty(2), Ty::Unknown);
}

#[test]
fn the_signature_closest_to_the_arguments_is_picked() {
    let with = |count: usize| {
        Signature::new(false, (0..count).map(|i| Param::new(&format!("p{i}"), Ty::NUMBER)).collect(), Ty::Void)
    };
    let function = Function { main: with(2), overloads: vec![with(0), with(4), with(1)], flags: 0 };
    assert_eq!(function.perfect_signature(2).params.len(), 2);
    assert_eq!(function.perfect_signature(0).params.len(), 0);
    assert_eq!(function.perfect_signature(1).params.len(), 1);
    assert_eq!(function.perfect_signature(3).params.len(), 2, "the first among equals");
    assert_eq!(function.perfect_signature(9).params.len(), 4);
}

#[test]
fn a_statement_finds_its_comment() {
    let text = "---@type number\nlocal a = 1\nlocal b = 2 ---@type string\nlocal c = 3\n\n---@class Foo\n\nlocal d = {\n    ---@type boolean\n    on = true,\n}\n";
    let src = Source::new(text);
    let comment = |needle: &str| {
        let at = text.find(needle).expect(needle);
        let node = src.root().tree().node_at(&src.text, at);
        let owner = std::iter::once(node)
            .chain(node.ancestors())
            .find(|node| node.kind().is_statement() || node.is(N::TableField));
        owner
            .and_then(|owner| src.comment_of(owner))
            .and_then(|comment| READER.declared(comment))
            .map(|ty| display(&ty))
    };
    assert_eq!(comment("local a").as_deref(), Some("number"));
    assert_eq!(comment("local b").as_deref(), Some("string"), "at the end of the line");
    assert_eq!(comment("local c"), None);
    assert_eq!(comment("local d"), None, "a blank line parts a comment from the statement");
    assert_eq!(comment("on = true").as_deref(), Some("boolean"));
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
#[ignore = "the stubs of the JetBrains plugin: CFXLUA_INTELLIJ=<its repository> cargo test --lib the_stubs_of_the_jetbrains_plugin_give_types -- --ignored --nocapture"]
fn the_stubs_of_the_jetbrains_plugin_give_types() {
    let repository = std::path::PathBuf::from(std::env::var("CFXLUA_INTELLIJ").expect("CFXLUA_INTELLIJ"));
    let mut stubs = Vec::new();
    lua_files(&repository.join("src/main/resources/std"), &mut stubs);
    stubs.sort();
    assert!(!stubs.is_empty());
    let (mut read, mut unknown) = (0, 0);
    let mut kinds: std::collections::BTreeMap<&'static str, usize> = std::collections::BTreeMap::new();
    for stub in &stubs {
        let text = std::fs::read_to_string(stub).expect("a stub").replace("\r\n", "\n");
        let src = Source::new(&text);
        for node in src.docs().descendants().filter(|node| node.kind().is_doc_type()) {
            let ty = READER.ty(node);
            let shown = display(&ty);
            assert!(!shown.is_empty(), "{} in {}", node.code(), stub.display());
            read += 1;
            unknown += usize::from(ty == Ty::Unknown && node.code() != "any" && node.code() != "unknown");
            *kinds.entry(node.kind().name()).or_default() += 1;
        }
    }
    eprintln!("{} stubs, {read} types read, {unknown} unknown that are not written `any`: {kinds:?}", stubs.len());
}
