use super::edges::{Family, anchors, of_comments};
use super::is_library;
use crate::lua::Source;
use crate::natives::Natives;

fn library(name: &str) -> bool {
    crate::lua::std::global(name).is_some() || matches!(name, "GetPlayerPed" | "Citizen")
}

/// The anchors of a text, each as its key and what it covers, in the order they are written.
fn read(text: &str) -> Vec<String> {
    let src = Source::new(text);
    anchors(&src, &library)
        .iter()
        .map(|anchor| format!("{} @{}", anchor.key, &text[anchor.start..anchor.end]))
        .collect()
}

#[test]
fn a_global_is_anchored_where_it_is_read_and_written() {
    let text =
        "Config = {}\nlocal copy = Config\nfunction Start() end\nStart(copy)\nprint(GetPlayerPed(-1), Citizen)\n";
    assert_eq!(
        read(text),
        [
            "lua.global#Config @Config",
            "lua.global#Config @Config",
            "lua.global#Start @Start",
            "lua.global#Start @Start"
        ],
        "not the local, not what a library declares"
    );
}

#[test]
fn a_local_of_the_name_of_a_global_is_not_one() {
    assert_eq!(read("local Config = {}\nConfig.Debug = true\nlocal function Start() end\nStart()\n"), [""; 0]);
    let text = "local Start\nfunction Start() end\nStop = Start\n";
    assert_eq!(read(text), ["lua.global#Stop @Stop"], "a function declared ahead is its local");
    assert_eq!(read("self.count = 1\nprint(self)\n"), [""; 0], "`self` outside a method is no global of the project");
}

#[test]
fn a_member_of_the_globals_table_is_the_global() {
    let text = "_G.Config = {}\nprint(_G['Config'], _G[name], _G.print)\n";
    assert_eq!(read(text), ["lua.global#Config @Config", "lua.global#Config @Config", "lua.global#name @name"]);
    assert_eq!(read("local _G = {}\n_G.Other = 1\n"), [""; 0], "a local named `_G` is a table like another");
}

#[test]
fn a_member_is_kept_under_the_path_it_is_read_through() {
    let text = "Config.Debug = true\nprint(Config.Sub.Level, Config['Debug'], Config[key], Config.Sub:reset())\n";
    assert_eq!(
        read(text),
        [
            "lua.global#Config @Config",
            "lua.member:Config#Debug @Debug",
            "lua.global#Config @Config",
            "lua.member:Config#Sub @Sub",
            "lua.member:Config.Sub#Level @Level",
            "lua.global#Config @Config",
            "lua.member:Config#Debug @Debug",
            "lua.global#Config @Config",
            "lua.global#key @key",
            "lua.global#Config @Config",
            "lua.member:Config#Sub @Sub",
            "lua.member:Config.Sub#reset @reset",
        ]
    );
    let dynamic = "print(Config[1].name, Config.list[i].name, get().name, ('x').len, string.format, Citizen.Wait)\n";
    assert_eq!(
        read(dynamic),
        [
            "lua.global#Config @Config",
            "lua.global#Config @Config",
            "lua.member:Config#list @list",
            "lua.global#i @i",
            "lua.global#get @get"
        ],
        "nothing after an index that is computed, a call, or a library"
    );
    let escaped = "print(Config['De\\x62ug'], Config[\"Debug\"], Config[ [[Debug]] ])\n";
    let found = read(escaped);
    assert_eq!(found.iter().filter(|anchor| anchor.starts_with("lua.member")).count(), 2, "{found:?}");
}

#[test]
fn a_method_and_its_self_are_members_of_what_the_method_is_declared_on() {
    let text = "function Shop:open()\n    self.count = self.count + 1\nend\nfunction Shop.Stock.clear() end\n";
    assert_eq!(
        read(text),
        [
            "lua.global#Shop @Shop",
            "lua.member:Shop#open @open",
            "lua.member:Shop#count @count",
            "lua.member:Shop#count @count",
            "lua.global#Shop @Shop",
            "lua.member:Shop#Stock @Stock",
            "lua.member:Shop.Stock#clear @clear",
        ]
    );
    assert_eq!(read("function self.a:b() return self.c end\n"), [""; 0], "an object named `self` leads nowhere");
    let nested = "function Shop:open()\n    local function inner() return self.count end\nend\n";
    assert_eq!(read(nested)[2..], ["lua.member:Shop#count @count"], "read from a function nested in the method");
    assert_eq!(
        read("function Shop.static() return self.count end\n").len(),
        2,
        "a function declared with a dot has no `self`"
    );
}

#[test]
fn the_fields_of_a_table_are_members_of_what_it_is_assigned_to() {
    let text = "Config = { Debug = true, Sub = { ['Level'] = 2, [1] = 'x', [key] = 1 }, 'plain' }\n";
    assert_eq!(
        read(text),
        [
            "lua.global#Config @Config",
            "lua.member:Config#Debug @Debug",
            "lua.member:Config#Sub @Sub",
            "lua.member:Config.Sub#Level @Level",
            "lua.global#key @key",
        ]
    );
    let other = "Config.Sub, Other = { a = 1 }, { b = 2 }\nlocal t = { c = 3 }\nreturn { d = 4 }, call({ e = 5 })\n";
    let found = read(other);
    let members: Vec<&String> = found.iter().filter(|anchor| anchor.starts_with("lua.member")).collect();
    assert_eq!(members, ["lua.member:Config#Sub @Sub", "lua.member:Config.Sub#a @a", "lua.member:Other#b @b"]);
}

#[test]
fn a_local_declared_as_a_class_keeps_its_members_under_the_class() {
    let text = "---@class Shop\nlocal Shop = { name = 'x' }\nfunction Shop:open() return self.name end\nShop.count = 0\nreturn Shop\n";
    assert_eq!(
        read(text),
        [
            "lua.class#Shop @Shop",
            "lua.member:Shop#name @name",
            "lua.member:Shop#open @open",
            "lua.member:Shop#name @name",
            "lua.member:Shop#count @count",
        ]
    );
    let plain = "local Shop = { name = 'x' }\nfunction Shop:open() return self.name end\n";
    assert_eq!(read(plain), [""; 0], "a local no comment declares stays in its file");
}

#[test]
fn the_classes_and_aliases_of_comments_are_anchored_by_their_name() {
    let text = "---@class Shop : Base\n---@field name string\n---@field owner Player|nil\n---@alias ShopId number\n\n---@generic T\n---@param shop Shop\n---@param value T\n---@return ShopId[]\nfunction open(shop, value) end\n";
    assert_eq!(
        read(text),
        [
            "lua.class#Shop @Shop",
            "lua.class#Base @Base",
            "lua.member:Shop#name @name",
            "lua.member:Shop#owner @owner",
            "lua.class#Player @Player",
            "lua.class#ShopId @ShopId",
            "lua.class#Shop @Shop",
            "lua.class#ShopId @ShopId",
            "lua.global#open @open",
        ],
        "not the types of the language, not a generic parameter"
    );
    let alone: Vec<String> = of_comments(text).iter().map(|anchor| anchor.key.clone()).collect();
    let both: Vec<String> = anchors(&Source::new(text), &library).iter().map(|anchor| anchor.key.clone()).collect();
    assert_eq!(alone, both[..both.len() - 1], "the comments read alone give the same, the code left out");
    assert!(of_comments("Shop = {} -- no annotation at all").is_empty());
}

#[test]
fn the_globals_of_a_module_file_are_members_of_its_module() {
    let text = "---@module utils\nlimit = 3\nfunction trim() return limit end\nprint(limit.max, other.limit)\n";
    assert_eq!(
        read(text),
        [
            "lua.class#utils @utils",
            "lua.member:utils#limit @limit",
            "lua.member:utils#trim @trim",
            "lua.member:utils#limit @limit",
            "lua.member:utils#limit @limit",
            "lua.member:utils.limit#max @max",
            "lua.member:utils#other @other",
            "lua.member:utils.other#limit @limit",
        ]
    );
    assert_eq!(read("print(utils.limit)\n"), ["lua.global#utils @utils", "lua.member:utils#limit @limit"]);
}

#[test]
fn each_anchor_covers_its_name_and_nothing_is_read_in_a_text_that_is_not_lua() {
    let text = "Config = { ['é'] = 1 }\nprint(Config['é'], Config.x)\n";
    let src = Source::new(text);
    let found = anchors(&src, &library);
    for anchor in &found {
        let name = anchor.key.rsplit_once('#').map(|(_, name)| name);
        assert_eq!(name, Some(&text[anchor.start..anchor.end]), "{}", anchor.key);
        assert!(anchor.start < anchor.end);
    }
    assert!(found.windows(2).all(|pair| pair[0].end <= pair[1].start), "in order, without overlap");
    assert_eq!(found.iter().filter(|anchor| anchor.family == Family::Member).count(), 3);
    let long = format!("{} = 1\n", "a".repeat(2000));
    assert_eq!(read(&long), [""; 0], "a key the host would refuse is not given");
    let deep = format!("local x = Config{}\n", ".a".repeat(600));
    assert!(Source::new(&deep).too_deep() && read(&deep).is_empty());
    assert!(anchors(&Source::plain("Config = 1"), &library).is_empty());
}

#[test]
fn the_names_of_the_libraries_are_told_from_those_of_a_project() {
    let natives = Natives::bundled();
    natives.catalog(crate::natives::NativeSet::Gta5);
    for name in ["print", "string", "Citizen", "GetPlayerPed", "exports"] {
        assert!(is_library(&natives, name), "{name}");
    }
    for name in ["Config", "ESX", "MyResource"] {
        assert!(!is_library(&natives, name), "{name}");
    }
    assert!(!is_library(&Natives::bundled(), "GetPlayerPed"), "a catalog that is not loaded is not read");
}

#[test]
fn a_project_gives_a_family_whole_or_not_at_all() {
    use super::{MOST_IN_A_PROJECT, families, kept};
    assert_eq!(families(3), [Family::Class, Family::Global, Family::Member]);
    assert_eq!(families(600), [Family::Class, Family::Global, Family::Member]);
    assert_eq!(families(601), [Family::Class, Family::Global]);
    assert_eq!(families(1200), [Family::Class, Family::Global]);
    assert_eq!(families(1201), [Family::Class]);
    let text = "---@class Shop\nShop = { stock = 1 }\nprint(Shop.stock)\n";
    let all = || anchors(&Source::new(text), &library);
    let families_of = |lua_files: usize, room: usize| {
        let mut found: Vec<Family> = kept(all(), lua_files, room).iter().map(|anchor| anchor.family).collect();
        found.dedup();
        found
    };
    assert_eq!(kept(all(), 3, MOST_IN_A_PROJECT).len(), 5);
    assert_eq!(families_of(900, MOST_IN_A_PROJECT), [Family::Class, Family::Global]);
    assert_eq!(families_of(5000, MOST_IN_A_PROJECT), [Family::Class]);
    assert_eq!(kept(all(), 3, 4).len(), 3, "the members go first, whole");
    assert_eq!(kept(all(), 3, 2).len(), 1, "then the globals");
    assert!(kept(all(), 3, 0).is_empty(), "a project that gave all it may gives nothing more");
    let many = format!("{}\n", "print(Config.a, Other)\n".repeat(1500));
    let found = kept(anchors(&Source::new(&many), &library), 3, MOST_IN_A_PROJECT);
    assert_eq!(found.len(), 3000, "4,500 anchors do not fit in a file: its members are left out");
    assert!(found.iter().all(|anchor| anchor.family == Family::Global));
    let more = format!("{}\n", "print(A, B, C)\n".repeat(1500));
    assert_eq!(kept(anchors(&Source::new(&more), &library), 3, MOST_IN_A_PROJECT).len(), 0, "nor do its globals");
}

/// How many anchors a real project gives: the files `CFXLUA_LIST` names under `CFXLUA_PROJECT`. Numbers only:
/// never a path, never text.
#[test]
#[ignore = "measurement: the anchors of the project given in CFXLUA_PROJECT and CFXLUA_LIST"]
fn anchors_of_a_real_project() {
    let (Ok(root), Ok(list)) = (std::env::var("CFXLUA_PROJECT"), std::env::var("CFXLUA_LIST")) else { return };
    let natives = Natives::bundled();
    for set in crate::natives::NativeSet::ALL {
        natives.catalog(set);
    }
    let listed = std::fs::read_to_string(list).expect("the list");
    let mut per_file: Vec<[usize; 3]> = Vec::new();
    let mut keys: [std::collections::HashSet<String>; 3] = Default::default();
    let mut spread: [std::collections::HashMap<String, (usize, std::collections::HashSet<usize>)>; 3] =
        Default::default();
    let mut depths: [Vec<usize>; 3] = Default::default();
    let (mut bytes, mut spent) = (0, std::time::Duration::ZERO);
    for line in listed.lines().filter(|line| line.ends_with(".lua")) {
        let Ok(text) = std::fs::read_to_string(std::path::Path::new(&root).join(line)) else { continue };
        if text.len() > crate::workspace::MAX_FILE {
            continue;
        }
        let src = Source::new(&text);
        let started = std::time::Instant::now();
        let found = anchors(&src, &|name| is_library(&natives, name));
        spent += started.elapsed();
        bytes += text.len();
        let mut counts = [0; 3];
        for anchor in found {
            counts[anchor.family as usize] += 1;
            let owner =
                anchor.key.split_once(':').and_then(|(_, rest)| rest.split_once('#')).map_or("", |(owner, _)| owner);
            depths[anchor.family as usize].push(if owner.is_empty() { 0 } else { owner.split('.').count() });
            let entry = spread[anchor.family as usize]
                .entry(anchor.key.clone())
                .or_insert((0usize, std::collections::HashSet::new()));
            entry.0 += 1;
            entry.1.insert(per_file.len());
            keys[anchor.family as usize].insert(anchor.key);
        }
        per_file.push(counts);
    }
    let total = |family: usize| per_file.iter().map(|counts| counts[family]).sum::<usize>();
    let all: Vec<usize> = per_file.iter().map(|counts| counts.iter().sum()).collect();
    let mut sorted = all.clone();
    sorted.sort_unstable();
    let at = |share: usize| sorted.get(sorted.len() * share / 100).copied().unwrap_or(0);
    eprintln!("{} files, {} KB, anchors read in {spent:?}", per_file.len(), bytes >> 10);
    for (family, name) in ["classes", "globals", "members"].iter().enumerate() {
        let most = per_file.iter().map(|counts| counts[family]).max().unwrap_or(0);
        eprintln!("  {name}: {} anchors, {} keys, {most} in one file at most", total(family), keys[family].len());
    }
    eprintln!(
        "  all: {} anchors; per file: median {}, 90% {}, 99% {}, most {}; files over 4096: {}, over 1000: {}",
        all.iter().sum::<usize>(),
        at(50),
        at(90),
        at(99),
        sorted.last().copied().unwrap_or(0),
        all.iter().filter(|count| **count > 4096).count(),
        all.iter().filter(|count| **count > 1000).count()
    );
    let without_members: usize = total(0) + total(1);
    eprintln!("  classes and globals alone: {without_members} anchors");
    for (family, name) in ["classes", "globals", "members"].iter().enumerate() {
        let mut uses: Vec<(usize, usize)> =
            spread[family].values().map(|(count, files)| (*count, files.len())).collect();
        uses.sort_unstable_by(|a, b| b.cmp(a));
        let covered = |most: usize| uses.iter().take(most).map(|(count, _)| count).sum::<usize>();
        let in_one_file = uses.iter().filter(|(_, files)| *files == 1);
        let (alone_keys, alone_anchors) =
            in_one_file.fold((0, 0), |(keys, anchors), (count, _)| (keys + 1, anchors + count));
        let depth = |deep: usize| depths[family].iter().filter(|d| **d >= deep).count();
        eprintln!(
            "  {name}: top 1 key {}, top 10 {}, top 100 {}, top 1000 {}; keys in one file only: {alone_keys} keys, {alone_anchors} anchors; owner of 2 names or more: {}, of 3 or more: {}",
            covered(1),
            covered(10),
            covered(100),
            covered(1000),
            depth(2),
            depth(3)
        );
    }
}
