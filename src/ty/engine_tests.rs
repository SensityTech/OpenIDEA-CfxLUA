//! The engine on small projects held in memory: the completion, generic, vararg and LuaCATS tests of the
//! JetBrains plugin, asked as "which members does the expression before the caret have".

use super::context::{Cx, Files, Options, Psi};
use super::expressions::assigned_name;
use super::{FileId, Function, Param, Ty, declarations, display, members, psi};
use crate::index::{Found, Index, MemberKind, declare};
use crate::lua::Source;
use crate::lua::tree::{N, Node};
use std::rc::Rc;

const CARET: &str = "--[[caret]]";
const LIBRARY: &str = include_str!("../lua/doc/testdata/synthlib.lua");

/// The class of the completion fixtures of the JetBrains plugin (`class.lua`).
const EMMY: (&str, &str) = (
    "class.lua",
    "---@class Emmy\nlocal e = { name = 'Emmy', age = 7 }\n\nfunction e:sayHello()\n    print('hi')\nend\n\n---@type Emmy\nemmy = nil\n",
);

struct Project {
    names: Vec<String>,
    sources: Vec<Rc<Source>>,
    index: Index,
    options: Options,
}

impl Files for Project {
    fn source(&self, file: FileId) -> Option<Rc<Source>> {
        self.sources.get(file as usize).cloned()
    }

    fn required(&self, path: &str) -> Option<FileId> {
        let wanted = format!("{}.lua", path.replace('.', "/"));
        self.names.iter().position(|name| *name == wanted).map(|file| file as FileId)
    }
}

impl Project {
    fn new(files: &[(&str, &str)]) -> Project {
        let mut project = Project {
            names: files.iter().map(|(name, _)| (*name).to_owned()).collect(),
            sources: files.iter().map(|(_, text)| Rc::new(Source::new(text))).collect(),
            index: Index::default(),
            options: Options::default(),
        };
        for file in 0..project.sources.len() {
            let declared = declare(&project.cx(), file as FileId, &project.sources[file]);
            project.index.set(file as FileId, declared);
        }
        project
    }

    /// The fixture class and `main`, the file the questions are asked in.
    fn with_emmy(main: &str) -> Project {
        Project::new(&[EMMY, ("main.lua", main)])
    }

    fn cx(&self) -> Cx<'_> {
        Cx::new(self, &self.index, &self.options)
    }

    /// The last file: the one the questions are asked in.
    fn main(&self) -> (FileId, &Source) {
        (self.sources.len() as FileId - 1, self.sources.last().expect("a file"))
    }

    fn psi<'s>(&'s self, node: Node<'s>) -> Psi<'s> {
        let (file, src) = self.main();
        Psi { file, src, node }
    }

    /// The node of kind `kind` that holds the byte `at` of the main file.
    fn node(&self, at: usize, kind: N) -> Node<'_> {
        let (_, src) = self.main();
        let inner = src.root().tree().node_at(&src.text, at);
        std::iter::once(inner).chain(inner.ancestors()).find(|node| node.is(kind)).expect("a node of that kind")
    }

    /// The type of the expression written before the `.` or the `:` that the caret mark follows.
    fn before_caret(&self) -> Ty {
        let (_, src) = self.main();
        let caret = src.text.find(CARET).expect("a caret mark");
        let sign = src.text[..caret].trim_end().len() - 1;
        assert!(matches!(&src.text[sign..sign + 1], "." | ":"), "the caret follows a member access");
        let prefix = psi::prefix(self.node(sign, N::IndexExpr)).expect("an expression before the sign");
        self.cx().infer(self.psi(prefix))
    }

    /// The names of the members a completion lists after the expression before the caret.
    fn members(&self) -> Vec<String> {
        let cx = self.cx();
        let ty = self.before_caret();
        let mut names: Vec<String> = Vec::new();
        for class in ty.top_classes() {
            for listed in members::visible(&cx, class) {
                if !names.iter().any(|name| name == listed.member.name()) {
                    names.push(listed.member.name().to_owned());
                }
            }
        }
        names
    }

    fn has(&self, wanted: &[&str]) -> bool {
        let members = self.members();
        wanted.iter().all(|name| members.iter().any(|member| member == name))
    }

    /// The type of the local `name` of the main file.
    fn local(&self, name: &str) -> Ty {
        let (_, src) = self.main();
        let def = src.root().descendants().find(|node| node.is(N::NameDef) && node.code() == name).expect(name);
        self.cx().infer(self.psi(def))
    }

    /// The names of the types of the local `name`, sorted; the type made for the local itself is left out.
    fn type_of(&self, name: &str) -> String {
        let ty = self.local(name);
        let mut names: Vec<String> = ty.each().iter().filter(|ty| !ty.is_anonymous()).map(display).collect();
        names.sort();
        names.dedup();
        names.join("|")
    }

    /// The function the first call of the main file calls.
    fn called(&self) -> Rc<Function> {
        let (_, src) = self.main();
        let call = src.root().descendants().find(|node| node.is(N::CallExpr)).expect("a call");
        let callee = self.cx().infer(self.psi(psi::prefix(call).expect("a callee")));
        let function = callee.each().iter().find_map(|ty| match ty {
            Ty::Function(function) => Some(function.clone()),
            _ => None,
        });
        function.expect("a function")
    }
}

fn shown(params: &[Param]) -> Vec<String> {
    params.iter().map(Param::display_name).collect()
}

#[test]
fn the_fields_of_a_global_table_and_what_is_assigned_to_it() {
    let globals = ("globals.lua", "gVar1 = 1\n\ngVar2 = {\n    aaa = 2,\n    bbb = 3\n}\ngVar2.ccc = 4\n");
    let project = Project::new(&[globals, ("test.lua", "gVar2.--[[caret]]")]);
    assert!(project.has(&["aaa", "bbb", "ccc"]), "{:?}", project.members());
    let number = Project::new(&[globals, ("test.lua", "local a = gVar1\nlocal b = gVar2.aaa\nlocal c = gVar2.ccc")]);
    assert_eq!(number.type_of("a"), "number");
    assert_eq!(number.type_of("b"), "number");
    assert_eq!(number.type_of("c"), "number");
}

#[test]
fn the_fields_and_methods_of_an_annotated_class() {
    let code = "---@class MyClass\n---@field public name string\nlocal s = {}\nfunction s:method()end\n\
                function s.staticMethod()end\n---@type MyClass\nlocal instance\n";
    let fields = Project::new(&[("test.lua", &format!("{code}instance.--[[caret]]"))]);
    assert!(fields.has(&["name", "method", "staticMethod"]), "{:?}", fields.members());
    let methods = Project::new(&[("test.lua", &format!("{code}instance:--[[caret]]"))]);
    assert!(methods.has(&["method"]));
    let typed = Project::new(&[("test.lua", &format!("{code}local n = instance.name\nlocal m = instance.method"))]);
    assert_eq!(typed.type_of("n"), "string");
    assert!(matches!(typed.local("m").each().first(), Some(Ty::Function(function)) if function.is_colon_call()));
}

#[test]
fn an_element_of_an_annotated_array() {
    let project =
        Project::with_emmy("---@type Emmy[]\nlocal emmyList = {}\n\nlocal emmy = emmyList[1]\nemmy.--[[caret]]");
    assert!(project.has(&["name", "age", "sayHello"]), "{:?}", project.members());
}

#[test]
fn what_an_annotated_function_returns() {
    let project =
        Project::with_emmy("---@type fun():Emmy\nlocal emmyCreator\n\nlocal emmy = emmyCreator()\nemmy.--[[caret]]");
    assert!(project.has(&["name", "age", "sayHello"]), "{:?}", project.members());
}

#[test]
fn the_values_a_generic_iterator_gives_to_a_loop() {
    let project = Project::with_emmy(
        "---@generic K, V\n---@param t table<K, V>|V[]\n---@return fun(tbl: table<K, V>):K, V\nfunction pairs(t) end\n\n\
         ---@type fun():table<string, Emmy>\nlocal emmyCreator\n\nfor k, emmy in pairs(emmyCreator()) do\n    emmy.--[[caret]]\nend",
    );
    assert!(project.has(&["name", "age", "sayHello"]), "{:?}", project.members());
}

#[test]
fn a_field_assigned_to_what_a_function_returns() {
    let project = Project::new(&[(
        "testAnonymous.lua",
        "local function test()\n    local v = xx()\n    v.pp = 123\n    return v\nend\nlocal v = test()\nv.--[[caret]]",
    )]);
    assert!(project.has(&["pp"]), "{:?}", project.members());
}

#[test]
fn a_table_written_in_a_return_annotation() {
    let project = Project::new(&[
        ("doc_table_test_A.lua", "---@return { name:string, value:number }\nfunction getData() end\n"),
        ("doc_table_test_B.lua", "local a = getData()\na.--[[caret]]"),
    ]);
    assert!(project.has(&["name", "value"]), "{:?}", project.members());
}

#[test]
fn self_is_what_the_method_is_declared_on() {
    let project = Project::new(&[(
        "testSelf.lua",
        "local s = {}\n\nfunction s:aaa()end\nfunction s:abb()end\nfunction s:ccc()\n    self.--[[caret]]\nend",
    )]);
    assert!(project.has(&["aaa", "abb", "ccc"]), "{:?}", project.members());
}

#[test]
fn the_globals_of_a_module_are_its_members() {
    let module = ("moduleA.lua", "---@module TypeA\nmodule(\"TypeA\")\n\nname = \"a\"\n\nfunction myFunction() end\n");
    let project = Project::new(&[module, ("B.lua", "local a ---@type TypeA\na.--[[caret]]")]);
    assert!(project.has(&["name", "myFunction"]), "{:?}", project.members());
    assert!(project.index.own("_G").next().is_none(), "nothing of the module is a global of the project");
    let required = Project::new(&[module, ("B.lua", "local a = require('moduleA')\na.--[[caret]]")]);
    assert!(required.has(&["name"]), "{:?}", required.members());
}

#[test]
fn a_method_that_returns_self_gives_the_class_it_is_called_on() {
    let project = Project::new(&[(
        "test_self.lua",
        "---@class A\n---@field aa string\nlocal a = {}\n\n---@return self\nfunction a:create()end\n\n\
         ---@class B : A\n---@field bb string\nlocal b = {}\n\nlocal foo = b:create()\nfoo.--[[caret]]",
    )]);
    assert!(project.has(&["bb", "aa", "create"]), "{:?}", project.members());
}

#[test]
fn each_value_of_a_tuple_has_its_type() {
    let project = Project::new(&[(
        "test_tuple_1.lua",
        "---@class Type1\nlocal obj = { name = \"name\" }\n\n---@return number, Type1\nlocal function getTuple()\nend\n\n\
         local a, b = getTuple()\nb.--[[caret]]",
    )]);
    assert!(project.has(&["name"]), "{:?}", project.members());
    assert_eq!(project.type_of("a"), "number");
    assert_eq!(project.type_of("b"), "Type1");
}

#[test]
fn a_closure_takes_the_parameters_its_place_asks_for() {
    let project = Project::with_emmy(
        "---@alias Handler fun(emmy: Emmy):void\n\n---@param func Handler\nlocal function test(func)\nend\n\n\
         test(function(value)\n    value.--[[caret]]\nend)",
    );
    assert!(project.has(&["sayHello"]), "{:?}", project.members());
}

#[test]
fn generic_parameters_are_read_from_the_arguments() {
    let returned = Project::with_emmy(
        "---@generic T\n---@param p1 T\n---@return T\nlocal function test(p1)\n    return p1\nend\n\n\
         local value = test(emmy)\nvalue.--[[caret]]",
    );
    assert!(returned.has(&["sayHello"]), "{:?}", returned.members());

    let given = Project::with_emmy(
        "---@generic T\n---@param p1 T\n---@param func fun(value: T):void\nlocal function test(p1, func)\nend\n\n\
         test(emmy, function(value)\n    value.--[[caret]]\nend)",
    );
    assert!(given.has(&["sayHello"]), "{:?}", given.members());

    let extended = Project::with_emmy(
        "---@class EmmyExt : Emmy\nlocal ext = {}\n\nfunction ext:reading() end\n\n---@generic T\n---@param p1 T\n\
         ---@param func fun(value: T):void\nlocal function test(p1, func)\nend\n\n\
         test(ext, function(value)\n    value.--[[caret]]\nend)",
    );
    assert!(extended.has(&["reading", "sayHello"]), "{:?}", extended.members());

    let iterator = Project::with_emmy(
        "---@generic T\n---@param list T[]\n---@return fun():number, T\nlocal function myIterator(list)\n    local idx = 0\n\
         \x20   return nil -- todo\nend\n\n---@type Emmy[]\nlocal emmyList = {}\n\n\
         for i, em in myIterator(emmyList) do\n    em.--[[caret]]\nend",
    );
    assert!(iterator.has(&["sayHello"]), "{:?}", iterator.members());
}

#[test]
fn extra_arguments_have_their_annotated_type() {
    let generic = Project::with_emmy(
        "---@generic T\n---@param index number|string\n---@vararg T\n---@return T\nlocal function select(index, ...)\nend\n\n\
         ---@type Emmy\nlocal emmy = {}\n\nlocal r = select(1, emmy, emmy, emmy)\nr.--[[caret]]",
    );
    assert!(generic.has(&["sayHello"]), "{:?}", generic.members());

    let packed =
        Project::with_emmy("---@vararg Emmy\nlocal function test(...)\n    local t = {...}\n    t[1].--[[caret]]\nend");
    assert!(packed.has(&["sayHello"]), "{:?}", packed.members());
}

#[test]
fn the_place_of_a_table_tells_what_its_fields_should_be() {
    let project = Project::new(&[(
        "test_issue_298.lua",
        "---@overload fun(t:{aaa: string, bbb: string, ccc: string})\n---@param aaa string\n---@param bbb string\n\
         ---@param ccc string\n---@return number\nfunction test(aaa, bbb, ccc)\nend\n\ntest {\n    a = 1\n}\n",
    )]);
    let (_, src) = project.main();
    let table = src.root().descendants().find(|node| node.is(N::TableExpr)).expect("the table");
    let expected = declarations::should_be(&project.cx(), project.psi(table));
    let mut fields: Vec<String> = Vec::new();
    for class in expected.top_classes() {
        fields.extend(members::visible(&project.cx(), class).iter().map(|listed| listed.member.name().to_owned()));
    }
    assert_eq!(fields, ["aaa", "bbb", "ccc"], "{expected:?}");
}

#[test]
fn the_place_of_an_argument_tells_which_strings_it_takes() {
    let project = Project::new(&[(
        "test_issue_338.lua",
        "---@alias Al '\"qwe\"' | '\"asd\"'\n\n---@class Qwe\n---@field a fun(self:self, args:Al):any\n\n\
         ---@type Qwe\nlocal zxc\n\nzxc:a(1)\n",
    )]);
    let (_, src) = project.main();
    let arg = src.root().descendants().find(|node| node.is(N::LiteralExpr)).expect("the argument");
    assert_eq!(display(&declarations::should_be(&project.cx(), project.psi(arg))), "\"qwe\"|\"asd\"");

    let literal = Project::new(&[(
        "test_literal.lua",
        "---@param type 'A' | \"B\"\nlocal function addListener(type, func)\nend\n\naddListener(1)\n",
    )]);
    let (_, src) = literal.main();
    let arg = src.root().descendants().find(|node| node.is(N::LiteralExpr)).expect("the argument");
    assert_eq!(display(&declarations::should_be(&literal.cx(), literal.psi(arg))), "\"A\"|\"B\"");
}

fn with_library(main: &str) -> Project {
    Project::new(&[("synthlib.lua", LIBRARY), ("main.lua", main)])
}

#[test]
fn optional_parameters_are_flagged_in_the_signature() {
    let signature = with_library("lib.requestModel('adder')").called().main.clone();
    assert_eq!(shown(&signature.params), ["model", "timeout?"]);
    assert_eq!(shown(&with_library("lib.showTextUI('x')").called().main.params), ["text", "position?", "options?"]);
}

#[test]
fn function_types_with_optional_parameters_and_extra_arguments() {
    let callback = with_library("lib.callback()").called();
    assert_eq!(shown(&callback.main.params), ["x?"]);
    assert_eq!(callback.main.vararg.as_ref().map(display).as_deref(), Some("string"));
    let project = with_library("local done = lib.callback()");
    assert_eq!(project.type_of("done"), "boolean");
    assert_eq!(with_library("lib.join('a', 'b')").called().main.vararg, Some(Ty::STRING), "`@param ... T`");
}

#[test]
fn several_return_tags_are_the_values_of_one_call() {
    let project = with_library("local ok, err = lib.checkDependency('x')\nlocal hash = lib.requestModel('adder')");
    assert_eq!(project.type_of("ok"), "boolean");
    assert_eq!(project.type_of("err"), "string");
    assert_eq!(project.type_of("hash"), "number");
}

#[test]
fn named_returns_on_one_line() {
    let project = with_library(
        "local count, handles = lib.count('ped')\nlocal sum, diff = lib.sumDiff(1, 2)\nlocal level, visible = lib.level()",
    );
    assert_eq!(project.type_of("count"), "number");
    assert_eq!(project.type_of("handles"), "number[]");
    assert_eq!(project.type_of("diff"), "number");
    assert_eq!(project.type_of("level"), "number");
    assert_eq!(project.type_of("visible"), "boolean");
}

#[test]
fn the_members_of_every_parent_class() {
    let project = with_library("local a = lib.on\nlocal b = lib.setTimeout\nlocal c = lib.notify");
    for name in ["a", "b", "c"] {
        assert!(project.local(name).each().iter().any(|ty| matches!(ty, Ty::Function(_))), "{name}");
    }
    let listed = with_library("lib.--[[caret]]");
    assert!(listed.has(&["on", "setTimeout", "notify", "points", "store"]), "{:?}", listed.members());
}

#[test]
fn index_signatures_type_the_fields_no_member_names() {
    let project = with_library(
        "local point = lib.store.anything\nlocal count = lib.store.count\nlocal key = 'k'\n\
         local other = lib.store[key]\nlocal dict = lib.points.any",
    );
    assert_eq!(project.type_of("point"), "SynthPoint");
    assert_eq!(project.type_of("count"), "number");
    assert_eq!(project.type_of("other"), "SynthPoint");
    assert_eq!(project.type_of("dict"), "SynthPoint");
}

#[test]
fn the_members_of_an_enum_have_the_type_of_their_value() {
    let project = with_library("local mode = SynthKeybindMode.HOLD\nlocal sky = SynthWeather.CLEAR");
    assert_eq!(project.type_of("mode"), "number");
    assert_eq!(project.type_of("sky"), "string");
}

#[test]
fn a_file_is_declared_again_when_it_changes() {
    let mut project = Project::new(&[("a.lua", "Shop = {}\nfunction Shop.open() end\nfunction Shop.close() end\n")]);
    let names = |project: &Project| {
        let mut names: Vec<String> = project.index.own("$Shop").map(|found| found.member.name.to_string()).collect();
        names.sort();
        names
    };
    assert_eq!(names(&project), ["close", "open"]);
    project.sources[0] = Rc::new(Source::new("Shop = {}\nfunction Shop.buy() end\n"));
    let declared = declare(&project.cx(), 0, &project.sources[0]);
    project.index.set(0, declared);
    assert_eq!(names(&project), ["buy"]);
    project.index.remove(0);
    assert!(names(&project).is_empty());
    assert!(project.index.own("_G").next().is_none());
}

/// The names of the types of the local `name`, the type made for the local itself left out.
fn types(project: &Project, name: &str) -> Vec<String> {
    project.type_of(name).split('|').map(str::to_owned).collect()
}

#[test]
fn a_cast_narrows_the_variable_read_after_it() {
    let project = with_library(
        "---@type string|number
local v = nil
local before = v
---@cast v string
local a = v
         ---@cast v +boolean
local b = v
---@cast v -string
local c = v
if a then
    ---@cast v SynthPoint
             local inner = v
end
local after = v
v = 1
local reassigned = v
",
    );
    assert_eq!(project.type_of("before"), "number|string");
    assert_eq!(project.type_of("a"), "string");
    assert_eq!(project.type_of("b"), "boolean|string");
    assert_eq!(project.type_of("c"), "boolean");
    assert_eq!(project.type_of("inner"), "SynthPoint");
    assert_eq!(project.type_of("after"), "boolean", "a cast written in a block ends with it");
    assert_eq!(project.type_of("reassigned"), "number|string", "an assignment ends the cast");
}

#[test]
fn a_cast_of_a_parameter_stays_inside_its_function() {
    let project = with_library(
        "---@param data any
local function handle(data)
    ---@cast data SynthNotifyProps
             local title = data.title
    return title
end
---@cast data SynthPoint
         local function other(data)
    local d = data
end
",
    );
    assert_eq!(project.type_of("title"), "string");
    assert!(!types(&project, "d").contains(&"SynthPoint".to_owned()), "{:?}", types(&project, "d"));
}

/// Functions that each return what each of the others returns, and the last one a number.
fn circle_of_functions(count: usize) -> String {
    let mut text = String::new();
    for rank in 0..count {
        text.push_str(&format!("function step{rank}(to)\n"));
        for other in (0..count).filter(|other| *other != rank) {
            text.push_str(&format!("    if to == {other} then return step{other}(to) end\n"));
        }
        text.push_str(if rank + 1 == count { "    return 7\nend\n" } else { "end\n" });
    }
    text.push_str("local reached = step0(1)\nlocal again = step5(1)\n");
    text
}

#[test]
fn functions_that_call_one_another_in_a_circle_are_each_read_once() {
    // An answer the circle cuts short is kept while the circle is walked: there are more paths through it than
    // can be walked.
    let text = circle_of_functions(12);
    let project = Project::new(&[("main.lua", &text)]);
    assert_eq!(project.type_of("reached"), "number");
    assert_eq!(project.type_of("again"), "number", "asked from elsewhere, the answer is complete");
}

#[test]
fn a_question_that_spends_its_steps_or_its_time_says_less() {
    let text = circle_of_functions(12);
    let project = Project::new(&[("main.lua", &text)]);
    let (_, src) = project.main();
    let reached = src.root().descendants().find(|node| node.is(N::NameDef) && node.code() == "reached");
    let reached = project.psi(reached.expect("the local"));
    let asked = |cx: Cx| (cx.infer(reached).each().contains(&Ty::NUMBER), cx.steps());
    let (complete, steps) = asked(project.cx());
    assert!(complete && (40..2_000).contains(&steps), "{steps} inferences");
    let minute = std::time::Duration::from_secs(60);
    assert_eq!(asked(project.cx().bounded(20, minute)), (false, 20), "past its steps nothing more is inferred");
    assert_eq!(asked(project.cx().bounded(steps, minute)), (true, steps), "with enough of them, all is");
    assert!(!asked(project.cx().bounded(u32::MAX, std::time::Duration::ZERO)).0, "nor past its time");
}

#[test]
fn fields_given_to_one_another_across_files_are_each_read_once() {
    // Each file gives each field of a global another field, and one of them a number: every assignment of a
    // field leads to every assignment of another. An answer of the circle is kept until the first question that
    // cut it is answered, not only until the last one is.
    let (fields, count) = (6, 6);
    let mut files: Vec<(String, String)> = vec![("globals.lua".to_owned(), "Shared = {}\n".to_owned())];
    for file in 0..count {
        let mut text = String::new();
        for field in 0..fields {
            let given = if file == 0 && field + 1 == fields {
                "7".to_owned()
            } else {
                format!("Shared.f{}", (field + file + 1) % fields)
            };
            text.push_str(&format!("Shared.f{field} = {given}\n"));
        }
        files.push((format!("file{file}.lua"), text));
    }
    files.push(("main.lua".to_owned(), "local reached = Shared.f0\n".to_owned()));
    let files: Vec<(&str, &str)> = files.iter().map(|(name, text)| (name.as_str(), text.as_str())).collect();
    assert_eq!(Project::new(&files).type_of("reached"), "number");
}

#[test]
fn what_has_no_type_has_no_member() {
    assert!(
        Project::new(&[(
            "t.lua",
            "local n = 1
n.--[[caret]]"
        )])
        .members()
        .is_empty()
    );
    assert!(Project::new(&[("t.lua", "Nope.--[[caret]]")]).members().is_empty());
    assert!(
        Project::new(&[(
            "t.lua",
            "local t = {}
t.--[[caret]]"
        )])
        .members()
        .is_empty()
    );
    let other = Project::new(&[
        (
            "a.lua",
            "local hidden = { secret = 1 }
",
        ),
        ("b.lua", "hidden.--[[caret]]"),
    ]);
    assert!(other.members().is_empty(), "a local of another file: {:?}", other.members());
}

#[test]
fn a_member_is_kept_under_the_classes_its_file_gives_it() {
    let project = Project::new(&[(
        "a.lua",
        "local M = {}
function M.open() end
M.count = 1

Shop = {}
function Shop:buy() end

         function Other.helper() end
function a.b.c:d() end
Other.field = 1
local n = 1
n.x = 2

         ---@class Bank
local bank = {}
function bank:pay() end
bank.rate = 2
",
    )]);
    let owners = |name: &str| {
        let declared = project.index.file(0).expect("the file");
        let member = declared.members.iter().find(|member| &*member.name == name).expect(name);
        member.owners.iter().map(|owner| owner.to_string()).collect::<Vec<_>>()
    };
    assert_eq!(owners("open"), ["0@(10)table", "6@0"], "the table of a local and the local itself");
    assert_eq!(owners("count"), ["0@(10)table", "6@0"]);
    assert_eq!(owners("buy"), ["0@(55)table", "$Shop"], "the table of a global and what its fields hold");
    assert_eq!(owners("helper"), ["$Other"], "a global the file does not assign");
    assert_eq!(owners("field"), ["$Other"]);
    assert_eq!(owners("d"), ["a.b.c"], "an owner the file says nothing of is named as it is written");
    assert_eq!(owners("pay"), ["Bank"], "an annotated class");
    assert_eq!(owners("rate"), ["Bank"]);
    assert_eq!(owners("Shop"), ["_G"]);
    let declared = project.index.file(0).expect("the file");
    assert!(declared.members.iter().all(|member| &*member.name != "x"), "a number has no field to declare");
    assert!(declared.members.iter().all(|member| &*member.name != "M"), "a local is no member");
    let bank = project.index.class("Bank").expect("the class");
    let table = project.main().1.text.find("local bank = ").expect("the table") + "local bank = ".len();
    assert_eq!(bank.alias, Some(format!("0@({table})table").into()), "the table the class annotates");
}

#[test]
#[ignore = "measurement: CFXLUA_PROJECT=<dir> (or CFXLUA_FILES=<file>;<file>) cargo test --release --lib declare_cost_of_real_files -- --ignored --nocapture"]
fn declare_cost_of_real_files() {
    let files: Vec<(String, String)> = match std::env::var("CFXLUA_FILES") {
        Ok(list) => {
            let paths = list.split(';').filter(|path| !path.is_empty());
            paths.map(|path| (path.to_owned(), std::fs::read_to_string(path).expect("a readable file"))).collect()
        }
        Err(_) => crate::testing::real_project().into_iter().filter(|(path, _)| path.ends_with(".lua")).collect(),
    };
    let listed = std::env::var("CFXLUA_FILES").is_ok();
    let mut project =
        Project { names: Vec::new(), sources: Vec::new(), index: Index::default(), options: Options::default() };
    let (mut bytes, mut parsing, mut declaring) = (0, std::time::Duration::ZERO, std::time::Duration::ZERO);
    let mut kinds: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let (mut members, mut classes, mut aliases, mut owners, mut slowest) = (0, 0, 0, 0, std::time::Duration::ZERO);
    for (file, (_, text)) in files.iter().enumerate() {
        if text.len() > crate::workspace::MAX_FILE {
            continue;
        }
        bytes += text.len();
        let start = std::time::Instant::now();
        let src = Rc::new(Source::new(text));
        src.root();
        src.docs();
        src.scopes();
        let parsed = start.elapsed();
        project.sources.push(src.clone());
        project.names.push(format!("{file}.lua"));
        let file = project.sources.len() as FileId - 1;
        let start = std::time::Instant::now();
        let declared = declare(&project.cx(), file, &src);
        let spent = start.elapsed();
        parsing += parsed;
        declaring += spent;
        slowest = slowest.max(spent);
        members += declared.members.len();
        classes += declared.classes.len();
        aliases += declared.aliases.len();
        owners += declared.members.iter().map(|member| member.owners.len()).sum::<usize>();
        for member in &declared.members {
            *kinds.entry(format!("{:?}", member.kind)).or_default() += 1;
        }
        if listed {
            eprintln!(
                "{} bytes: parsed in {parsed:?}, declared in {spent:?}: {} members, {} classes",
                text.len(),
                declared.members.len(),
                declared.classes.len()
            );
        }
        project.index.set(file, declared);
    }
    eprintln!(
        "{} files, {bytes} bytes: parsed in {parsing:?}, declared in {declaring:?} (slowest file {slowest:?})",
        project.sources.len()
    );
    eprintln!("{members} members under {owners} class names, {classes} classes, {aliases} aliases: {kinds:?}");
}

/// For each member of the main file that keeps its type, that type and the one reading the file gives.
fn kept_and_read(project: &Project) -> Vec<(String, String, String)> {
    let cx = project.cx();
    let (file, _) = project.main();
    let declared = project.index.file(file).expect("the file");
    let mut out = Vec::new();
    for member in declared.members.iter().filter(|member| member.ty.is_some() && member.kind != MemberKind::DocField) {
        let found = Found { file, member };
        let read = match member.kind {
            MemberKind::Global => cx.at(file, member.node, |def| assigned_name(&cx, def)),
            _ => cx.at(file, member.node, |psi| cx.with_value(0, || cx.infer(psi))),
        };
        let kept = members::declared_type(&cx, found);
        out.push((member.name.to_string(), display(&kept), display(&read.expect("the node"))));
    }
    out
}

#[test]
fn the_type_a_file_keeps_for_a_member_is_the_one_reading_it_gives() {
    let shapes = Project::new(&[(
        "shapes.lua",
        "local M = {}\nM.count = 1\nM.name = 'shop'\nM.open = true\nM.none = nil\nM.items = {}\n\
         M.buy = function(item, amount) return amount end\nM.computed = M.count + 1\nM.called = tostring(1)\n\
         ---@type Shop[]\nM.all = fetch()\n---@param id number\n---@return Shop\nfunction M.find(id) end\n\
         function M:close(...) end\n\nConfig = { debug = false, .ready, ['key'] = 2, rate = 1.5, list = { 1 }, cb = print }\n\
         ---@class Shop\nShop = {}\n---@type number\nLimit = compute()\nFlag = true\nLater = compute()\na, b = 1\n\
         ---@class Bank\nBank = make()\n",
    )]);
    let kept = kept_and_read(&shapes);
    for (name, kept, read) in &kept {
        assert_eq!(kept, read, "{name}");
    }
    let names: Vec<&str> = kept.iter().map(|(name, _, _)| name.as_str()).collect();
    for stated in ["count", "name", "open", "none", "items", "buy", "all", "find", "close"] {
        assert!(names.contains(&stated), "{stated} says its type: {names:?}");
    }
    for stated in ["Config", "Shop", "Limit", "Flag", "Bank", "a"] {
        assert!(names.contains(&stated), "{stated} says its type: {names:?}");
    }
    for read in ["computed", "called", "Later", "b"] {
        assert!(!names.contains(&read), "{read} has to be inferred: {names:?}");
    }
    for field in ["debug", "ready", "key", "rate", "list", "cb"] {
        assert!(!names.contains(&field), "{field} is written in a table: {names:?}");
    }
    let shown = |wanted: &str| kept.iter().find(|(name, _, _)| name == wanted).map(|(_, kept, _)| kept.as_str());
    assert_eq!(shown("buy"), Some("fun(item: any, amount: any): any"));
    assert_eq!(shown("find"), Some("fun(id: number): Shop"));
    assert_eq!(shown("all"), Some("Shop[]"));
    assert_eq!(shown("Limit"), Some("number"));
    assert_eq!(shown("Shop"), Some("Shop|table|[global Shop]"));
    assert_eq!(shown("Bank"), Some("[global Bank]|Bank"), "a union comes before the type it is joined to");

    for (name, kept, read) in kept_and_read(&Project::new(&[("synthlib.lua", LIBRARY)])) {
        assert_eq!(kept, read, "{name}");
    }
}

#[test]
fn the_fields_a_table_writes_are_read_in_its_file() {
    let config = (
        "config.lua",
        "Config = { debug = false, .ready, ['key'] = 2, rate = 1.5, list = { 1 }, cb = print, 'positional' }
         Config.added = 'later'
",
    );
    let listed = Project::new(&[config, ("main.lua", "Config.--[[caret]]")]);
    let mut members = listed.members();
    members.sort();
    assert_eq!(members, ["added", "cb", "debug", "key", "list", "rate", "ready"], "no positional value");
    assert!(listed.index.file(0).expect("the file").members.iter().all(|member| &*member.name != "debug"));
    let typed = Project::new(&[
        config,
        (
            "main.lua",
            "local d = Config.debug
local r = Config.ready
local k = Config.key
local l = Config.list
",
        ),
    ]);
    assert_eq!(typed.type_of("d"), "boolean");
    assert_eq!(typed.type_of("r"), "boolean", "a set field holds true");
    assert_eq!(typed.type_of("k"), "number");
    assert_eq!(display(&typed.local("l")), "table|[local l]");
    let nested = Project::new(&[
        (
            "config.lua",
            "Config = { shop = { items = { bread = { price = 2 } } } }
",
        ),
        ("main.lua", "Config.shop.items.bread.--[[caret]]"),
    ]);
    assert_eq!(nested.members(), ["price"]);
}
