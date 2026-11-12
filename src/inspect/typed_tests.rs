use super::*;
use crate::editor::EditorFeature;
use crate::feature::Feature;
use crate::settings::Settings;
use crate::testing::Fixture;

const MANIFEST: (&str, &str) =
    ("shop/fxmanifest.lua", "fx_version 'cerulean'\ngame 'gta5'\nshared_script 'shared/*.lua'\n");
const MAIN: &str = "shop/shared/main.lua";

/// What one type safety inspection, switched on, reports in `text`, as `text: message`.
fn found(text: &str, inspection: Inspection) -> Vec<String> {
    let fx = Fixture::new(&[MANIFEST, (MAIN, text)])
        .with_settings(Settings::defaults().with(inspection.setting, true.into()));
    let feature = EditorFeature::default();
    fx.index(&feature);
    let reported = feature.problems(&fx.cx(&fx.doc(MAIN)));
    let kept = reported.iter().filter(|problem| problem.code == inspection.code);
    kept.map(|p| format!("{}: {}", &text[p.start..p.end], p.message)).collect()
}

const TWO: &str = "---@param p1 number\n---@param p2 string\nlocal function test(p1, p2)\nend\n";

#[test]
fn an_argument_is_of_the_type_its_parameter_asks_for() {
    // `MatchFunctionSignatureTest`
    assert_eq!(
        found(&format!("{TWO}test(1, 2)"), MATCH_SIGNATURE),
        ["2: Type mismatch. Required: 'string' Found: 'number'"]
    );
    assert_eq!(found(&format!("{TWO}test(1, \"2\", 3)"), MATCH_SIGNATURE), ["3: Too many arguments."]);
    assert_eq!(found("local function test(p1, p2)\nend\ntest(1)", MATCH_SIGNATURE), ["): Missing argument: p2: any"]);
    assert!(found(&format!("{TWO}test(1, 'x')\ntest(nil, nil)"), MATCH_SIGNATURE).is_empty(), "nil fits anything");
}

#[test]
fn the_last_argument_gives_all_its_values_and_the_others_their_first() {
    // `MatchFunctionSignatureTest.testMultiReturn` and `testParentIndex`
    let text = "local function ret_nn()\n    return 1, 2\nend\nlocal function ret_sn()\n    return \"1\", 2\nend\n\
                ---@param n1 number\n---@param n2 number\nlocal function acp_nn(n1, n2) end\n\
                acp_nn(ret_nn())\nacp_nn(ret_sn())\nacp_nn(ret_nn(), 1)\nacp_nn(ret_sn(), 1)";
    assert_eq!(found(text, MATCH_SIGNATURE), ["ret_sn(): Type mismatch. Required: 'number' Found: 'string'"; 2]);
    let index = "local dummy, A = 1, {}\n---@return number, string\nfunction A.a()\n    return 1, \"1\"\nend\n\
                 ---@param n number\n---@param s string\nlocal function acp_ns(n, s) end\nacp_ns(A.a())\nprint(dummy)";
    assert!(found(index, MATCH_SIGNATURE).is_empty());
}

#[test]
fn unions_overloads_arrays_and_optional_parameters_are_matched() {
    // `typeSafety.lua`
    let text = "---@param x number | nil\nfunction test2(x) end\ntest2(3)\ntest2(nil)\ntest2(\"\")\n\
                ---@overload fun(x: number, y: string): boolean\n---@param x number\nfunction test3(x) end\n\
                test3(3)\ntest3(3, \"4\")\ntest3(\"\")\n\
                ---@param x number[]\nfunction testList(x) end\n---@type number[]\nlocal list = {1, 2, 3}\ntestList(list)\n\
                ---@type string[]\nlocal names = {}\ntestList(names)\ntestList({})";
    assert_eq!(
        found(text, MATCH_SIGNATURE),
        [
            "\"\": Type mismatch. Required: 'number' Found: 'string'",
            "\"\": Type mismatch. Required: 'number' Found: 'string'",
            "names: Type mismatch. Required: 'number[]' Found: 'string[]'",
        ]
    );
    // `LuaCatsInspectionsTest`, "optional parameters may be omitted"
    let lib = "lib = {}\n---@param name string\n---@param timeout? number\nfunction lib.requestModel(name, timeout) end\n\
               ---@alias Side 'top'|'left'\n---@param text string\n---@param side? Side\nfunction lib.show(text, side) end\n\
               lib.requestModel('adder')\nlib.requestModel('adder', 5000)\nlib.show('E')\nlib.show('E', 'top')\n\
               local position = 'left'\nlib.show('E', position)\nlib.requestModel()";
    assert_eq!(found(lib, MATCH_SIGNATURE), ["): Missing argument: name: string"]);
}

#[test]
fn a_call_to_what_a_class_does_not_declare_is_unknown() {
    let text =
        "---@class Shop\nShop = {}\nfunction Shop:open() end\nShop:open()\nShop:close()\nShop.sell()\nother.thing()";
    assert_eq!(
        found(text, MATCH_SIGNATURE),
        [
            "Shop:close(): Unknown function 'close'.",
            "Shop.sell(): Unknown function 'sell'.",
            "other.thing(): Unknown function 'thing'.",
        ],
        "a global nothing declares has no member either"
    );
}

#[test]
fn a_return_gives_what_its_function_says_it_returns() {
    // `ReturnTypeInspectionTest`
    assert_eq!(
        found("---@return string\nlocal function test()\n    return 1\nend\ntest()", RETURN_TYPE),
        ["return 1: Type mismatch. Expected: 'string' Found: 'number'"]
    );
    assert!(
        found("---@return string\nlocal function test()\n    return \"right\"\nend\ntest()", RETURN_TYPE).is_empty()
    );
    assert_eq!(
        found("---@return string\nlocal function test()\nend\ntest()", RETURN_TYPE),
        ["()\nend: Return type 'string' specified but no return values found."]
    );
    assert_eq!(
        found("---@return number, string\nfunction pair()\n    return 1\nend", RETURN_TYPE),
        ["1: Incorrect number of return values. Expected 2 but found 1."]
    );
    assert_eq!(
        found("---@return number\nfunction one()\n    return\nend", RETURN_TYPE),
        ["return: Type mismatch. Expected: 'number' Found: 'nil'"]
    );
    assert!(
        found("function free()\n    return 1, 'a'\nend\nreturn free", RETURN_TYPE).is_empty(),
        "nothing is declared"
    );
}

#[test]
fn an_assignment_gives_a_value_of_the_type_of_its_target() {
    let text = "---@class Shop\n---@field name string\nShop = {}\nShop.name = 'x'\nShop.name = 1\n\
                local count = 1\ncount = 'two'\ncount = 3\nlocal a, b\na, b = 1\na = 1, 2\nprint(a, b)";
    assert_eq!(
        found(text, ASSIGN_TYPE),
        [
            "1: Type mismatch. Required: 'string' Found: 'number'",
            "'two': Type mismatch. Required: 'number' Found: 'string'",
            "b: Missing value assignment.",
            "2: Nothing to assign to.",
        ]
    );
}

#[test]
fn a_name_nothing_declares_is_undeclared() {
    // `UndeclaredVariableTest`
    assert_eq!(
        found("print(MyUndeclaredVariable)", UNDECLARED),
        ["MyUndeclaredVariable: Undeclared variable 'MyUndeclaredVariable'."]
    );
    let known =
        "local a = 1\ncount = 2\nfunction Shop() end\nprint(a, count, Shop, _G, string.format('%d', a), vector3)";
    assert!(found(known, UNDECLARED).is_empty(), "locals, globals of the file, Lua and the Cfx runtime");
}

#[test]
fn type_safety_is_off_until_it_is_asked_for() {
    let text = format!("{TWO}test(1, 2)\nprint(Unknown)\ncount = 1\ncount = 'two'");
    let fx = Fixture::new(&[MANIFEST, (MAIN, &text)]);
    let feature = EditorFeature::default();
    fx.index(&feature);
    let codes: Vec<&str> = feature.problems(&fx.cx(&fx.doc(MAIN))).iter().map(|problem| problem.code).collect();
    assert!(TYPED.iter().all(|inspection| !codes.contains(&inspection.code)), "{codes:?}");
    assert!(TYPED.iter().all(|inspection| !inspection.on_by_default()));
    assert!(ALL.iter().chain(&INDEXED).all(|inspection| inspection.on_by_default()));
}
