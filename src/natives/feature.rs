use super::convert::cmp_ignore_case;
use super::model::Native;
use super::parse::longest_backtick_run;
use super::{NativeSet, client_sets, update, visible_sets};
use crate::feature::{CommandCx, CommandResult, Ctx, Feature, Problem};
use crate::lua::Source;
use crate::lua::lexer::Tok;
use crate::lua::syntax::Seg;
use crate::matching::{Match, Query};
use crate::project::{FileKind, Game, Side};
use crate::settings::keys;
use openidea_plugin::Severity;
use openidea_plugin::providers::{CompletionItem, CompletionKind};
use std::collections::HashSet;
use std::fmt::Write;

const MAX_ITEMS: usize = 2000;
const MAX_DOCUMENTED: usize = 500;

pub const WRONG_SIDE: &str = "CfxNativeWrongSide";

#[derive(Default)]
pub struct NativesFeature;

impl Feature for NativesFeature {
    fn command(&self, cx: &CommandCx, id: &str, _arg: Option<&str>) -> Option<CommandResult> {
        match id {
            update::UPDATE => Some(update::run(cx)),
            update::SCHEDULED_UPDATE => Some(update::scheduled(cx)),
            _ => None,
        }
    }

    fn completion(&self, cx: &Ctx, at: usize, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if !cx.settings.enabled() || cx.doc.kind != FileKind::Lua || !cx.doc.is_lua() {
            return Vec::new();
        }
        let Some(start) = word_start(&cx.doc.src, at) else { return Vec::new() };
        let query = Query::new(&cx.doc.text()[start..at]);
        let mut sets = visible_sets(cx.settings, &cx.ws.detected_games(), cx.doc.side, resource_games(cx));
        // lua-language-server completes the natives of its definitions: listed twice otherwise.
        sets.retain(|s| !cx.ws.luals.sets.contains(s));
        let show_aliases = cx.settings.bool(keys::SHOW_DEPRECATED, false);
        let show_unnamed = cx.settings.bool(keys::SHOW_UNNAMED, false);
        let mut seen: HashSet<&str> = HashSet::new();
        let mut found: Vec<(Match, &Native)> = Vec::new();
        for set in sets {
            for n in &cx.natives.catalog(set).natives {
                if (n.is_alias() && !show_aliases) || (is_unnamed(n) && !show_unnamed) {
                    continue;
                }
                // A native of several sets (CFX `shared`) is offered once, from the first.
                if let Some(m) = query.matches(&n.lua_name)
                    && seen.insert(&n.lua_name)
                {
                    found.push((m, n));
                }
            }
        }
        // Exact, prefix, then camel-hump matches; primaries and named natives
        // first; shorter names (the everyday natives) before longer ones.
        found.sort_by(|(ma, a), (mb, b)| {
            ma.cmp(mb)
                .then(a.is_alias().cmp(&b.is_alias()))
                .then(is_unnamed(a).cmp(&is_unnamed(b)))
                .then(a.lua_name.len().cmp(&b.lua_name.len()))
                .then_with(|| cmp_ignore_case(&a.lua_name, &b.lua_name))
                .then_with(|| a.lua_name.cmp(&b.lua_name))
        });
        found.truncate(MAX_ITEMS);
        found.into_iter().enumerate().map(|(rank, (_, n))| item(n, rank, rank < MAX_DOCUMENTED)).collect()
    }

    fn problems(&self, cx: &Ctx) -> Vec<Problem> {
        wrong_side(cx)
    }
}

fn resource_games<'a>(cx: &Ctx<'a>) -> Option<&'a [Game]> {
    cx.doc.resource.as_deref().and_then(|dir| cx.ws.resource(dir)).and_then(|r| r.games.as_deref())
}

fn is_unnamed(n: &Native) -> bool {
    n.lua_name.starts_with("N_0x")
}

fn in_string(src: &Source, at: usize) -> bool {
    let i = src.all.partition_point(|t| t.start < at);
    let Some(t) = i.checked_sub(1).map(|i| src.all[i]) else { return false };
    let open = match t.kind {
        Tok::Str { closed } | Tok::Hash { closed } => !closed,
        Tok::LongStr => !t.text(&src.text).ends_with(']'),
        _ => return false,
    };
    at < t.end || (at == t.end && open)
}

fn word_start(src: &Source, at: usize) -> Option<usize> {
    let text = src.text.as_str();
    if at > text.len() || !text.is_char_boundary(at) {
        return None;
    }
    let bytes = text.as_bytes();
    let mut start = at;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    if start == at || bytes[start].is_ascii_digit() || src.in_comment(at) || in_string(src, at) {
        return None;
    }
    if let Some(t) = src.tok_before(start).map(|i| src.toks[i]) {
        let s = t.text(text);
        let blocked = match t.kind {
            Tok::Op => matches!(s, "." | ":" | "::"),
            Tok::Keyword => matches!(s, "local" | "function" | "goto"),
            Tok::Number => s.ends_with('.'),
            _ => false,
        };
        if blocked {
            return None;
        }
    }
    Some(start)
}

fn item(n: &Native, rank: usize, documented: bool) -> CompletionItem {
    let signature = n.typed_signature();
    let mut item = CompletionItem::new(&n.lua_name, CompletionKind::Function);
    item.detail = signature.get(n.lua_name.len()..).map(str::to_owned);
    item.sort_text = Some(format!("{rank:04}"));
    item.documentation = documented.then(|| markdown(n));
    item
}

fn one_line(text: &str) -> String {
    text.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
}

fn entry(out: &mut String, name: &str, lua: &str, native: &str, description: Option<&str>, note: Option<&str>) {
    let _ = write!(out, "- `{name}: {lua}` — `{native}`");
    if let Some(d) = description.map(one_line).filter(|d| !d.is_empty()) {
        let _ = write!(out, " – {d}");
    }
    if let Some(note) = note {
        let _ = write!(out, " {note}");
    }
    out.push('\n');
}

pub fn markdown(n: &Native) -> String {
    let mut s = String::with_capacity(512 + n.description.len() + n.lua_example.as_ref().map_or(0, String::len));
    let _ = write!(s, "```lua\nfunction {}\n```\n", n.typed_signature());
    if let Some(primary) = &n.alias_of {
        let _ = write!(s, "\n**Deprecated**: Deprecated alias: use `{primary}` instead.\n");
    }
    if !n.description.is_empty() {
        let _ = write!(s, "\n{}\n", n.description);
    }
    if let Some(example) = &n.lua_example {
        let fence = "`".repeat(longest_backtick_run(example).max(2) + 1);
        let _ = write!(s, "\n{fence}lua\n{example}\n{fence}\n");
    }
    if n.lua_params().next().is_some() {
        s.push_str("\n**Params:**\n\n");
        for p in n.lua_params() {
            let note = p.kept_trailing_pointer.then_some("(pointer kept in the call, also returned)");
            entry(&mut s, &p.name, &p.lua_type, &p.native_type, p.description.as_deref(), note);
        }
    }
    if !n.returns.is_empty() {
        s.push_str("\n**Returns:**\n\n");
        for r in &n.returns {
            let out_pointer = n.params.iter().any(|p| p.out_pointer && p.name == r.name);
            let note = out_pointer.then_some("(out-pointer: returned, not passed)");
            entry(&mut s, &r.name, &r.lua_type, &r.native_type, r.description.as_deref(), note);
        }
    }
    let _ = write!(
        s,
        "\n- **Namespace:** `{}`\n- **Side:** {}\n- **Game:** {}\n- **Hash:** `{}`\n",
        n.namespace,
        n.side_label(),
        n.game_label(),
        n.hash_display()
    );
    match &n.original_name {
        Some(name) => {
            let _ = writeln!(s, "- **Native name:** `{name}`");
        }
        None => s.push_str("- **Native name:** Unnamed (hash only)\n"),
    }
    if n.rpc {
        s.push_str("- **RPC:** Server-side RPC of a client native (OneSync)\n");
    }
    if !n.aliases.is_empty() {
        let aliases: Vec<String> = n.aliases.iter().map(|a| format!("`{a}`")).collect();
        let _ = writeln!(s, "- **Deprecated aliases:** {}", aliases.join(", "));
    }
    if let Some(alt) = &n.alt_name {
        let _ = writeln!(s, "- **NativeDB name:** `{alt}`");
    }
    if !n.doc_url.is_empty() {
        let _ = write!(s, "\n[Online documentation]({})\n", n.doc_url);
    }
    s
}

fn declared_names(src: &Source) -> HashSet<&str> {
    fn names<'a>(src: &'a Source, mut j: usize, out: &mut HashSet<&'a str>) {
        while src.is_name(j) {
            out.insert(src.tok_text(j));
            j += 1;
            if src.is_op(j, "<") {
                j += 3;
            }
            if !src.is_op(j, ",") {
                break;
            }
            j += 1;
        }
    }
    let mut out = HashSet::new();
    for i in 0..src.toks.len() {
        if src.is_kw(i, "function") {
            let mut j = i + 1;
            if src.is_name(j) && !src.is_op(j + 1, ".") && !src.is_op(j + 1, ":") {
                out.insert(src.tok_text(j));
            }
            while src.is_name(j) || src.is_op(j, ".") || src.is_op(j, ":") {
                j += 1;
            }
            if src.is_op(j, "(") {
                names(src, j + 1, &mut out);
            }
        } else if (src.is_kw(i, "local") && src.is_name(i + 1)) || src.is_kw(i, "for") {
            names(src, i + 1, &mut out);
        } else if src.is_name(i) && src.is_op(i + 1, "=") && src.is_statement_start(i) {
            out.insert(src.tok_text(i));
        }
    }
    out
}

fn wrong_side(cx: &Ctx) -> Vec<Problem> {
    let doc = cx.doc;
    if !cx.settings.enabled() || doc.kind != FileKind::Lua || !doc.is_lua() {
        return Vec::new();
    }
    let clients = client_sets(cx.settings, &cx.ws.detected_games(), resource_games(cx));
    let server = [NativeSet::Server];
    let (own, other, native_side) = match doc.side {
        Side::Client => (clients.as_slice(), &server[..], "server"),
        Side::Server => (&server[..], clients.as_slice(), "client"),
        _ => return Vec::new(),
    };
    let src = &doc.src;
    let declared = declared_names(src);
    let defines = |sets: &[NativeSet], name: &str| sets.iter().any(|&s| cx.natives.catalog(s).get(name).is_some());
    let mut out = Vec::new();
    for chain in src.chains() {
        let called = matches!(chain.segs.first(), Some(Seg::Call { .. } | Seg::StrCall { .. } | Seg::TableCall { .. }));
        let name = chain.head.as_str();
        if !called
            || (chain.start > 0 && src.is_kw(chain.start - 1, "function"))
            || declared.contains(name)
            || defines(own, name)
            || !defines(other, name)
        {
            continue;
        }
        let t = src.toks[chain.start];
        let message = format!("'{name}' is a {native_side} native: {} scripts cannot call it", doc.side.id());
        out.push(Problem::new(WRONG_SIDE, Severity::Warning, t.start, t.end, message));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::natives::{Param, Return};
    use crate::settings::Settings;
    use crate::testing::{Fixture, native};
    use serde_json::Value;

    const MANIFEST: &str = "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client/*.lua'\n\
                            server_script 'server/*.lua'\nshared_script 'shared/*.lua'\n";

    fn natives() -> Vec<Native> {
        let mut alias = native(NativeSet::Gta5, "GetRaycastResult", Side::Client);
        alias.alias_of = Some("GetShapeTestResult".into());
        let mut shape = native(NativeSet::Gta5, "GetShapeTestResult", Side::Client);
        shape.aliases = vec!["GetRaycastResult".into()];
        let mut unnamed = native(NativeSet::Gta5, "N_0x0b40ed49d7d6ff84", Side::Client);
        unnamed.original_name = None;
        vec![
            native(NativeSet::Gta5, "GetEntityCoords", Side::Client),
            native(NativeSet::Gta5, "GetGroundZFor_3dCoord", Side::Client),
            native(NativeSet::Gta5, "GetEntityHeading", Side::Client),
            native(NativeSet::Gta5, "Get", Side::Client),
            native(NativeSet::Gta5, "RegisterCommand", Side::Shared),
            native(NativeSet::Gta5, "PlayerPedId", Side::Client),
            shape,
            alias,
            unnamed,
            native(NativeSet::Rdr3, "GetRdrOnly", Side::Client),
            native(NativeSet::Server, "GetPlayerIdentifiers", Side::Server),
            native(NativeSet::Server, "RegisterCommand", Side::Shared),
        ]
    }

    fn fixture(rel: &str, text: &str) -> Fixture {
        Fixture::new(&[("res/fxmanifest.lua", MANIFEST), (rel, text)]).with_natives(natives())
    }

    fn labels(fx: &Fixture, rel: &str) -> Vec<String> {
        let (doc, at) = fx.caret(rel);
        NativesFeature.completion(&fx.cx(&doc), at, None).into_iter().map(|i| i.label).collect()
    }

    fn problems(fx: &Fixture, rel: &str) -> Vec<(String, String)> {
        let doc = fx.doc(rel);
        NativesFeature
            .problems(&fx.cx(&doc))
            .into_iter()
            .map(|p| (doc.text()[p.start..p.end].to_owned(), p.message))
            .collect()
    }

    #[test]
    fn client_completion_follows_side_game_and_settings() {
        let fx = fixture("res/client/main.lua", "local c = Get<caret>");
        assert_eq!(
            labels(&fx, "res/client/main.lua"),
            ["Get", "GetEntityCoords", "GetEntityHeading", "GetShapeTestResult", "GetGroundZFor_3dCoord"],
            "exact match first, then shorter names; no server native, alias or RedM native"
        );
        let fx = fixture("res/client/main.lua", "N_0x<caret>");
        assert!(labels(&fx, "res/client/main.lua").is_empty(), "unnamed natives are hidden");
        let settings = Settings::defaults()
            .with(keys::SHOW_DEPRECATED, Value::from(true))
            .with(keys::SHOW_UNNAMED, Value::from(true));
        let fx = fixture("res/client/main.lua", "GetRay<caret>").with_settings(settings);
        assert_eq!(labels(&fx, "res/client/main.lua"), ["GetRaycastResult"]);
        let settings = Settings::defaults().with(keys::SHOW_UNNAMED, Value::from(true));
        let fx = fixture("res/client/main.lua", "N_0x<caret>").with_settings(settings);
        assert_eq!(labels(&fx, "res/client/main.lua"), ["N_0x0b40ed49d7d6ff84"]);
        let fx = fixture("res/client/main.lua", "Get<caret>")
            .with_settings(Settings::defaults().with("enabled", Value::from(false)));
        assert!(labels(&fx, "res/client/main.lua").is_empty());
    }

    #[test]
    fn natives_of_lua_language_server_definitions_are_left_to_it() {
        let only_gta = crate::luals::definitions(
            &crate::natives::Natives::from_natives(Vec::new()),
            &[NativeSet::Gta5],
            &[crate::runtime::Part::Shared],
        )
        .0;
        let mut fx = Fixture::new(&[
            ("res/fxmanifest.lua", "fx_version 'cerulean'\ngames { 'gta5', 'rdr3' }\nclient_script 'client/*.lua'\n"),
            ("res/client/main.lua", "local c = Get<caret>"),
            (".cfxlua/natives.lua", &only_gta),
        ])
        .with_natives(natives());
        assert!(labels(&fx, "res/client/main.lua").contains(&"GetEntityCoords".to_owned()), "no .luarc.json yet");
        let luarc = fx.ws.absolute(crate::luals::LUARC);
        fx.ws.set(&luarc, Some(include_str!("../../templates/luarc.json")));
        assert_eq!(labels(&fx, "res/client/main.lua"), ["GetRdrOnly"], "only the game the definitions lack");
    }

    #[test]
    fn sides_and_shared_natives() {
        let fx = fixture("res/server/main.lua", "Get<caret>");
        assert_eq!(labels(&fx, "res/server/main.lua"), ["GetPlayerIdentifiers"]);
        let fx = fixture("res/shared/config.lua", "Re<caret>");
        assert_eq!(labels(&fx, "res/shared/config.lua"), ["RegisterCommand"], "a shared native is offered once");
        let fx = fixture("res/shared/config.lua", "Get<caret>");
        assert!(labels(&fx, "res/shared/config.lua").contains(&"GetPlayerIdentifiers".to_owned()));
        let fx = Fixture::new(&[("res/fxmanifest.lua", "game 'gta5'\nclient_script 'x.lua'\nGet<caret>")])
            .with_natives(natives());
        assert!(labels(&fx, "res/fxmanifest.lua").is_empty(), "never in manifests");
        let fx = Fixture::new(&[
            ("rdr/fxmanifest.lua", "game 'rdr3'\nclient_script '*.lua'"),
            ("rdr/cl.lua", "GetR<caret>"),
        ])
        .with_natives(natives());
        assert_eq!(labels(&fx, "rdr/cl.lua"), ["GetRdrOnly"], "the resource's game");
    }

    #[test]
    fn only_where_a_native_can_be_written() {
        for text in [
            "local p = ped.Get<caret>",
            "local p = ped:Get<caret>",
            "local p = ped?.Get<caret>",
            "print('Get<caret>')",
            "print(\"Get<caret>",
            "-- Get<caret>",
            "--[[ Get<caret> ]]",
            "local Get<caret>",
            "function Get<caret>",
            "local x = [[Get<caret>]]",
            "local x = 1<caret>",
            "local x = <caret>",
        ] {
            let fx = fixture("res/client/main.lua", text);
            assert!(labels(&fx, "res/client/main.lua").is_empty(), "{text}");
        }
        let fx = fixture("res/client/main.lua", "print('x') Get<caret>");
        assert!(!labels(&fx, "res/client/main.lua").is_empty());
    }

    #[test]
    fn camel_humps_and_ranking() {
        let fx = fixture("res/client/main.lua", "ggz<caret>");
        assert_eq!(labels(&fx, "res/client/main.lua"), ["GetGroundZFor_3dCoord"]);
        let fx = fixture("res/client/main.lua", "gec<caret>");
        assert_eq!(labels(&fx, "res/client/main.lua"), ["GetEntityCoords", "GetGroundZFor_3dCoord"], "Ge(t) + C(oord)");
        let fx = fixture("res/client/main.lua", "ppi<caret>");
        assert_eq!(labels(&fx, "res/client/main.lua"), ["PlayerPedId"]);
        let fx = fixture("res/client/main.lua", "getentityc<caret>");
        assert_eq!(labels(&fx, "res/client/main.lua"), ["GetEntityCoords"], "case-insensitive prefix");
        let fx = fixture("res/client/main.lua", "ec<caret>");
        assert!(labels(&fx, "res/client/main.lua").is_empty(), "the first word is anchored");
    }

    #[test]
    fn items_carry_signature_and_documentation() {
        let mut coords = native(NativeSet::Gta5, "GetEntityCoords", Side::Client);
        coords.params = vec![
            Param {
                name: "entity".into(),
                lua_type: "integer".into(),
                native_type: "Entity".into(),
                ..Param::default()
            },
            Param { name: "alive".into(), lua_type: "boolean".into(), native_type: "BOOL".into(), ..Param::default() },
        ];
        coords.returns = vec![Return {
            name: "retval".into(),
            lua_type: "vector3".into(),
            native_type: "Vector3".into(),
            description: None,
        }];
        let fx = fixture("res/client/main.lua", "GetEntityCo<caret>").with_natives(vec![coords]);
        let (doc, at) = fx.caret("res/client/main.lua");
        let items = NativesFeature.completion(&fx.cx(&doc), at, None);
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!((item.label.as_str(), item.insert_text.as_str()), ("GetEntityCoords", "GetEntityCoords"));
        assert_eq!(item.detail.as_deref(), Some("(entity: integer, alive: boolean): vector3"));
        assert_eq!(item.kind, Some(CompletionKind::Function));
        assert!(
            item.documentation
                .as_deref()
                .is_some_and(|d| d.starts_with("```lua\nfunction GetEntityCoords(entity: integer"))
        );
    }

    #[test]
    fn markdown_documentation() {
        let mut n = native(NativeSet::Gta5, "GetGroundZFor_3dCoord", Side::Client);
        n.original_name = Some("GET_GROUND_Z_FOR_3D_COORD".into());
        n.hash = "0xc906a7dab05c8d2b".into();
        n.namespace = "MISC".into();
        n.description = "Gets the ground.".into();
        n.lua_example = Some("local ok, z = GetGroundZFor_3dCoord(x, y, z, false)\n-- ```".into());
        n.doc_url = "https://docs.fivem.net/natives/?_0xC906A7DAB05C8D2B".into();
        n.aliases = vec!["GetGroundZ".into(), "N_0xc906a7dab05c8d2b".into()];
        n.params = vec![
            Param {
                name: "x".into(),
                lua_type: "number".into(),
                native_type: "float".into(),
                description: Some("Position\non X.".into()),
                ..Param::default()
            },
            Param {
                name: "groundZ".into(),
                lua_type: "number".into(),
                native_type: "float*".into(),
                out_pointer: true,
                ..Param::default()
            },
            Param {
                name: "p".into(),
                lua_type: "integer|nil".into(),
                native_type: "Any*".into(),
                kept_trailing_pointer: true,
                ..Param::default()
            },
        ];
        n.returns = vec![
            Return {
                name: "retval".into(),
                lua_type: "boolean".into(),
                native_type: "BOOL".into(),
                description: Some("found".into()),
            },
            Return {
                name: "groundZ".into(),
                lua_type: "number".into(),
                native_type: "float*".into(),
                description: None,
            },
        ];
        let md = markdown(&n);
        let expected = "```lua\nfunction GetGroundZFor_3dCoord(x: number, p: integer|nil): boolean, number\n```\n\n\
            Gets the ground.\n\n\
            ````lua\nlocal ok, z = GetGroundZFor_3dCoord(x, y, z, false)\n-- ```\n````\n\n\
            **Params:**\n\n\
            - `x: number` — `float` – Position on X.\n\
            - `p: integer|nil` — `Any*` (pointer kept in the call, also returned)\n\n\
            **Returns:**\n\n\
            - `retval: boolean` — `BOOL` – found\n\
            - `groundZ: number` — `float*` (out-pointer: returned, not passed)\n\n\
            - **Namespace:** `MISC`\n- **Side:** Client\n- **Game:** FiveM\n- **Hash:** `0xC906A7DAB05C8D2B`\n\
            - **Native name:** `GET_GROUND_Z_FOR_3D_COORD`\n\
            - **Deprecated aliases:** `GetGroundZ`, `N_0xc906a7dab05c8d2b`\n\n\
            [Online documentation](https://docs.fivem.net/natives/?_0xC906A7DAB05C8D2B)\n";
        assert_eq!(md, expected);

        let mut alias = native(NativeSet::Server, "SetPlayerModelOld", Side::Server);
        alias.original_name = None;
        alias.alias_of = Some("SetPlayerModel".into());
        alias.rpc = true;
        alias.alt_name = Some("_SET_MODEL".into());
        let md = markdown(&alias);
        assert!(md.contains("\n**Deprecated**: Deprecated alias: use `SetPlayerModel` instead.\n"), "{md}");
        assert!(md.contains("- **Side:** Server\n- **Game:** FiveM + RedM\n"), "{md}");
        assert!(md.contains(
            "- **Native name:** Unnamed (hash only)\n- **RPC:** Server-side RPC of a client native (OneSync)\n"
        ));
        assert!(md.contains("- **NativeDB name:** `_SET_MODEL`\n"));
        assert!(!md.contains("Online documentation"), "no link without a URL");
    }

    #[test]
    fn wrong_side_calls() {
        let client = "local ids = GetPlayerIdentifiers(src)\nRegisterCommand('x', function() end)\n\
                      local c = GetEntityCoords(PlayerPedId())\nobj.GetPlayerIdentifiers()\nobj:GetPlayerIdentifiers()\n\
                      GetPlayerIdentifiers 'str'\nprint(GetPlayerIdentifiers)";
        let fx = fixture("res/client/main.lua", client);
        let found = problems(&fx, "res/client/main.lua");
        let message = "'GetPlayerIdentifiers' is a server native: client scripts cannot call it";
        assert_eq!(found, vec![("GetPlayerIdentifiers".to_owned(), message.to_owned()); 2]);

        let fx =
            fixture("res/server/main.lua", "local c = GetEntityCoords(GetPlayerPed(src))\nRegisterCommand('x', cb)");
        let found = problems(&fx, "res/server/main.lua");
        assert_eq!(
            found,
            [(
                "GetEntityCoords".to_owned(),
                "'GetEntityCoords' is a client native: server scripts cannot call it".to_owned()
            )]
        );
        let doc = fx.doc("res/server/main.lua");
        let p = &NativesFeature.problems(&fx.cx(&doc))[0];
        assert_eq!((p.code, p.severity, p.fixes.len()), (WRONG_SIDE, Severity::Warning, 0));

        for declared in [
            "local function GetPlayerIdentifiers(s) end\nGetPlayerIdentifiers(1)",
            "function GetPlayerIdentifiers(s) end\nGetPlayerIdentifiers(1)",
            "local GetPlayerIdentifiers <const> = exports.x.ids\nGetPlayerIdentifiers(1)",
            "GetPlayerIdentifiers = function() end\nGetPlayerIdentifiers(1)",
            "local f = function(a, GetPlayerIdentifiers) return GetPlayerIdentifiers(a) end",
            "for _, GetPlayerIdentifiers in pairs(t) do GetPlayerIdentifiers() end",
        ] {
            let fx = fixture("res/client/main.lua", declared);
            assert!(problems(&fx, "res/client/main.lua").is_empty(), "{declared}");
        }
        for rel in ["res/shared/main.lua", "res/other.lua", "res/fxmanifest.lua"] {
            let fx = fixture(rel, "GetPlayerIdentifiers(src)\nGetEntityCoords(ped)");
            assert!(problems(&fx, rel).is_empty(), "{rel}");
        }
        let fx = fixture("res/client/main.lua", "GetPlayerIdentifiers(src)")
            .with_settings(Settings::defaults().with("enabled", Value::from(false)));
        assert!(problems(&fx, "res/client/main.lua").is_empty());
        let off = Settings::defaults().with(keys::SERVER_NATIVES, Value::from(false));
        let fx = fixture("res/server/main.lua", "RegisterCommand('x', cb)").with_settings(off);
        assert!(problems(&fx, "res/server/main.lua").is_empty(), "a shared native, whatever the switches");
    }

    #[test]
    fn real_catalog_completion() {
        let mut all = crate::natives::tests::bundled(NativeSet::Gta5).to_vec();
        all.extend_from_slice(crate::natives::tests::bundled(NativeSet::Server));
        let fx = fixture("res/shared/main.lua", "S<caret>").with_natives(all);
        let (doc, at) = fx.caret("res/shared/main.lua");
        let start = std::time::Instant::now();
        let items = NativesFeature.completion(&fx.cx(&doc), at, None);
        let elapsed = start.elapsed();
        let bytes: usize =
            items.iter().map(|i| i.label.len() + i.detail.as_ref().map_or(0, String::len)).sum::<usize>()
                + items.iter().filter_map(|i| i.documentation.as_ref()).map(String::len).sum::<usize>();
        eprintln!("'S': {} items, {} KiB, in {} ms", items.len(), bytes / 1024, elapsed.as_millis());
        assert!(items.len() > 1700 && items.len() <= MAX_ITEMS);
        assert_eq!(items.iter().filter(|i| i.documentation.is_some()).count(), MAX_DOCUMENTED);
        assert!(items.iter().all(|i| i.label.starts_with(['S', 's'])));
        if !cfg!(debug_assertions) {
            assert!(elapsed.as_millis() < 25, "{elapsed:?}");
        }
        let fx = fixture("res/client/main.lua", "GetEntityCoo<caret>")
            .with_natives(crate::natives::tests::bundled(NativeSet::Gta5).to_vec());
        let (doc, at) = fx.caret("res/client/main.lua");
        let items = NativesFeature.completion(&fx.cx(&doc), at, None);
        assert_eq!(items[0].label, "GetEntityCoords");
        let md = items[0].documentation.as_deref().unwrap_or_default();
        assert!(
            md.starts_with("```lua\nfunction GetEntityCoords(entity: integer, alive: boolean): vector3\n```\n"),
            "{md}"
        );
        assert!(
            md.contains("- **Native name:** `GET_ENTITY_COORDS`\n") && md.contains("- **Hash:** `0x3FEF770D40960D5A`")
        );
    }
}
