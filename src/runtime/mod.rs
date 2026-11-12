pub mod stubs;

use crate::feature::{Ctx, Feature};
use crate::lua::Source;
use crate::project::Side;
use crate::settings::{Settings, keys};
use openidea_plugin::providers::{CompletionItem, CompletionKind};
use std::collections::HashSet;
pub use stubs::{Decl, Kind, Part, Runtime, Typed, runtime};

#[derive(Default)]
pub struct RuntimeFeature;

pub fn visible_parts(settings: &Settings, side: Side) -> Vec<Part> {
    if !settings.enabled() || !settings.bool(keys::RUNTIME_API, true) || side == Side::Manifest {
        return Vec::new();
    }
    let mut parts = vec![Part::Shared];
    if settings.bool(keys::CLIENT_NATIVES, true) && Side::Client.visible_from(side) {
        parts.push(Part::Client);
    }
    if settings.bool(keys::SERVER_NATIVES, true) && Side::Server.visible_from(side) {
        parts.push(Part::Server);
    }
    parts
}

impl Feature for RuntimeFeature {
    fn completion(&self, cx: &Ctx, at: usize, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let doc = cx.doc;
        let mut parts = visible_parts(cx.settings, doc.side);
        parts.retain(|p| !cx.ws.luals.parts.contains(p));
        if !doc.is_lua() || parts.is_empty() || doc.src.in_literal(at) {
            return Vec::new();
        }
        let text = doc.text();
        let start = text[..at].bytes().rposition(|b| !(b.is_ascii_alphanumeric() || b == b'_')).map_or(0, |i| i + 1);
        let prefix = &text[start..at];
        if prefix.starts_with(|c: char| c.is_ascii_digit()) {
            return Vec::new();
        }
        let rt = runtime();
        let src = &doc.src;
        let prev = src.tok_before(start).filter(|&p| src.toks[p].end <= start);
        match prev.map(|p| (p, src.tok_text(p))) {
            Some((p, ".")) => member_items(rt, &parts, src, p, false),
            Some((p, ":")) => member_items(rt, &parts, src, p, true),
            Some((_, "local" | "function" | "goto" | "::")) => Vec::new(),
            _ => global_items(rt, &parts, prefix),
        }
    }
}

fn global_items(rt: &Runtime, parts: &[Part], prefix: &str) -> Vec<CompletionItem> {
    let prefix = prefix.to_ascii_lowercase();
    let mut items: Vec<CompletionItem> = rt
        .globals()
        .filter(|(name, g)| {
            !g.extension && parts.contains(&rt.decl(g.decl).part) && name.to_ascii_lowercase().starts_with(&prefix)
        })
        .map(|(_, g)| item(rt, rt.decl(g.decl)))
        .collect();
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

enum Ty<'a> {
    Table(String),
    Function(&'a Decl),
}

fn member_items(rt: &Runtime, parts: &[Part], src: &Source, op: usize, colon: bool) -> Vec<CompletionItem> {
    let Some(Ty::Table(table)) = expression_type(rt, parts, src, op) else { return Vec::new() };
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for &m in rt.members(&table) {
        let d = rt.decl(m);
        if d.colon == colon && parts.contains(&d.part) && seen.insert(d.name.as_str()) {
            items.push(item(rt, d));
        }
    }
    if let Some(class) = rt.class(&table).filter(|c| !colon && parts.contains(&c.part)) {
        for f in &class.doc.fields {
            if seen.insert(f.name.as_str()) {
                items.push(field_item(class, f));
            }
        }
    }
    items
}

fn expression_type<'a>(rt: &'a Runtime, parts: &[Part], src: &Source, op: usize) -> Option<Ty<'a>> {
    enum Seg<'s> {
        Member(&'s str),
        Call,
    }
    let mut segs = Vec::new();
    let mut k = op.checked_sub(1)?;
    let head = loop {
        if src.is_op(k, "?") {
            k = k.checked_sub(1)?;
        }
        if src.is_name(k) {
            let name = src.tok_text(k);
            match k.checked_sub(1).map(|p| src.tok_text(p)) {
                Some("." | ":") => {
                    segs.push(Seg::Member(name));
                    k = k.checked_sub(2)?;
                }
                _ => break name,
            }
        } else if src.is_op(k, ")") {
            segs.push(Seg::Call);
            k = src.pair[k]?.checked_sub(1)?;
        } else {
            return None;
        }
    };
    let visible = |d: &Decl| parts.contains(&d.part);
    let g = rt.global(head).filter(|g| visible(rt.decl(g.decl)))?;
    let mut ty = decl_type(rt, rt.decl(g.decl), g.table.as_deref())?;
    for seg in segs.into_iter().rev() {
        ty = match (seg, ty) {
            (Seg::Member(name), Ty::Table(table)) => {
                let member = rt.members(&table).iter().map(|&m| rt.decl(m)).find(|d| d.name == name && visible(d));
                match member {
                    Some(d) => decl_type(rt, d, None)?,
                    None => {
                        let field = rt.class(&table)?.doc.fields.iter().find(|f| f.name == name)?;
                        Ty::Table(rt.table_of(&field.ty)?.to_owned())
                    }
                }
            }
            (Seg::Call, Ty::Function(f)) => Ty::Table(rt.table_of(&f.doc.returns.first()?.ty)?.to_owned()),
            _ => return None,
        };
    }
    Some(ty)
}

fn decl_type<'a>(rt: &Runtime, d: &'a Decl, table: Option<&str>) -> Option<Ty<'a>> {
    if d.is_function() {
        return Some(Ty::Function(d));
    }
    let table = table.or_else(|| d.var_type().and_then(|t| rt.table_of(t)))?;
    Some(Ty::Table(table.to_owned()))
}

fn item(rt: &Runtime, d: &Decl) -> CompletionItem {
    let kind = match d.kind {
        Kind::Function => CompletionKind::Function,
        Kind::Method => CompletionKind::Method,
        Kind::Field => CompletionKind::Field,
        Kind::Class => CompletionKind::Class,
        Kind::Global if d.doc.class.is_some() => CompletionKind::Class,
        Kind::Global | Kind::Local => CompletionKind::Variable,
    };
    let mut item = CompletionItem::new(&d.name, kind);
    item.detail = Some(signature(rt, d));
    item.documentation = Some(markdown(rt, d));
    item
}

fn field_item(class: &Decl, f: &Typed) -> CompletionItem {
    let mut item = CompletionItem::new(&f.name, CompletionKind::Field);
    let owner = class.doc.class.as_deref().unwrap_or(&class.name);
    let signature = format!("{owner}.{}: {}", f.name, f.ty);
    let mut doc = format!("```lua\n{signature}\n```");
    if !f.desc.is_empty() {
        doc.push_str("\n\n");
        doc.push_str(&f.desc);
    }
    doc.push_str(&format!("\n\n*{}*", class.part.display_name()));
    item.detail = Some(signature);
    item.documentation = Some(doc);
    item
}

pub fn signature(rt: &Runtime, d: &Decl) -> String {
    if d.is_function() {
        let params: Vec<String> = d.typed_params().iter().map(|p| format!("{}: {}", p.name, p.ty)).collect();
        let mut s = format!("{}({})", d.qualified(), params.join(", "));
        if !d.doc.returns.is_empty() {
            let returns: Vec<&str> = d.doc.returns.iter().map(|r| r.ty.as_str()).collect();
            s.push_str(": ");
            s.push_str(&returns.join(", "));
        }
        return s;
    }
    if let Some(class) = &d.doc.class {
        return format!("class {class}");
    }
    match d.var_type() {
        Some(ty) => format!("{}: {ty}", d.qualified()),
        None if rt.members(&d.name).is_empty() => d.qualified(),
        None => format!("{}: table", d.qualified()),
    }
}

pub fn markdown(rt: &Runtime, d: &Decl) -> String {
    let a = &d.doc;
    let mut out = format!("```lua\n{}{}\n```", if d.is_function() { "function " } else { "" }, signature(rt, d));
    if let Some(msg) = &a.deprecated {
        out.push_str(&format!("\n\n**Deprecated**{}", if msg.is_empty() { String::new() } else { format!(": {msg}") }));
    }
    if !a.description.is_empty() {
        out.push_str("\n\n");
        out.push_str(&a.description);
    }
    let list = |title: &str, items: Vec<String>| {
        if items.is_empty() { String::new() } else { format!("\n\n**{title}**\n\n{}", items.join("\n")) }
    };
    let entry = |name: &str, ty: &str, desc: &str| {
        let mut s = if name.is_empty() { format!("- `{ty}`") } else { format!("- `{name}` `{ty}`") };
        if !desc.is_empty() {
            s.push_str(": ");
            s.push_str(desc);
        }
        s
    };
    out.push_str(&list("Fields", a.fields.iter().map(|f| entry(&f.name, &f.ty, &f.desc)).collect()));
    if d.is_function() {
        out.push_str(&list("Parameters", d.typed_params().iter().map(|p| entry(&p.name, &p.ty, &p.desc)).collect()));
    }
    out.push_str(&list("Returns", a.returns.iter().map(|r| entry("", &r.ty, &r.desc)).collect()));
    out.push_str(&list("Overloads", a.overloads.iter().map(|o| format!("- `{o}`")).collect()));
    out.push_str(&format!("\n\n*{}*", d.part.display_name()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Fixture;
    use serde_json::Value;

    const MANIFEST: &str = "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client/*.lua'\nserver_script 'server/*.lua'\nshared_script 'shared/*.lua'";

    fn complete_with(settings: Settings, rel: &str, code: &str) -> Vec<CompletionItem> {
        let fx = Fixture::new(&[("res/fxmanifest.lua", MANIFEST), (rel, code)]).with_settings(settings);
        let (doc, at) = fx.caret(rel);
        RuntimeFeature.completion(&fx.cx(&doc), at, None)
    }

    fn labels(rel: &str, code: &str) -> Vec<String> {
        complete_with(Settings::defaults(), rel, code).into_iter().map(|i| i.label).collect()
    }

    fn has(list: &[String], name: &str) -> bool {
        list.iter().any(|l| l == name)
    }

    #[test]
    fn globals_by_side() {
        let client = labels("res/client/main.lua", "<caret>");
        let expected = [
            "Citizen",
            "CreateThread",
            "AddEventHandler",
            "TriggerServerEvent",
            "SendNUIMessage",
            "json",
            "vector3",
            "LocalPlayer",
            "GlobalState",
        ];
        for name in expected {
            assert!(has(&client, name), "{name} in client");
        }
        for name in ["TriggerClientEvent", "source", "PerformHttpRequest", "fx_version", "math", "os", "PromiseType"] {
            assert!(!has(&client, name), "{name} not in client");
        }
        let server = labels("res/server/main.lua", "<caret>");
        assert!(
            has(&server, "TriggerClientEvent")
                && has(&server, "source")
                && has(&server, "Wait")
                && has(&server, "GetPlayers")
        );
        assert!(!has(&server, "TriggerServerEvent") && !has(&server, "SendNUIMessage") && !has(&server, "LocalPlayer"));
        let shared = labels("res/shared/config.lua", "<caret>");
        assert!(has(&shared, "TriggerClientEvent") && has(&shared, "TriggerServerEvent"));
        let fx = Fixture::new(&[("res/fxmanifest.lua", "fx_<caret>")]);
        let (doc, at) = fx.caret("res/fxmanifest.lua");
        assert!(RuntimeFeature.completion(&fx.cx(&doc), at, None).is_empty(), "manifests: the manifest feature");
        assert_eq!(labels("res/client/main.lua", "local x = Cit<caret>"), ["Citizen"]);
        assert!(labels("res/client/main.lua", "local Cit<caret>").is_empty());
        assert!(labels("res/client/main.lua", "-- Cit<caret>").is_empty());
        assert!(labels("res/client/main.lua", "print('Cit<caret>')").is_empty());
    }

    #[test]
    fn settings_switch_parts() {
        let off = Settings::defaults().with(keys::RUNTIME_API, Value::from(false));
        assert!(complete_with(off, "res/client/main.lua", "<caret>").is_empty());
        let no_client = Settings::defaults().with(keys::CLIENT_NATIVES, Value::from(false));
        let shared: Vec<String> =
            complete_with(no_client, "res/shared/a.lua", "<caret>").into_iter().map(|i| i.label).collect();
        assert!(
            has(&shared, "CreateThread") && has(&shared, "TriggerClientEvent") && !has(&shared, "TriggerServerEvent")
        );
        let disabled = Settings::defaults().with(keys::ENABLED, Value::from(false));
        assert!(complete_with(disabled, "res/client/main.lua", "<caret>").is_empty());
    }

    #[test]
    fn members_of_tables_classes_and_calls() {
        let citizen = labels("res/client/main.lua", "Citizen.<caret>");
        assert!(has(&citizen, "CreateThread") && has(&citizen, "Wait") && has(&citizen, "GetNative"));
        assert!(!has(&labels("res/server/main.lua", "Citizen.Cr<caret>"), "GetNative"));
        let json = labels("res/server/main.lua", "local s = json.<caret>");
        assert!(has(&json, "encode") && has(&json, "decode") && has(&json, "null"));
        let mut methods = labels("res/client/main.lua", "local p = promise.new():<caret>");
        methods.sort();
        assert_eq!(methods, ["next", "reject", "resolve"]);
        let promise = labels("res/client/main.lua", "promise.<caret>");
        assert!(has(&promise, "new") && has(&promise, "all") && !has(&promise, "resolve"));
        assert_eq!(labels("res/server/main.lua", "Player(source).state:<caret>"), ["set"]);
        assert_eq!(labels("res/client/main.lua", "LocalPlayer.<caret>"), ["state"]);
        assert!(
            labels("res/client/main.lua", "GlobalState.<caret>").is_empty(),
            "state bag keys belong to the state bags feature"
        );
        assert!(labels("res/client/main.lua", "LocalPlayer.state.<caret>").is_empty());
        let math = labels("res/client/main.lua", "math.<caret>");
        assert!(has(&math, "round") && has(&math, "clamp") && has(&math, "eps"));
        assert!(has(&labels("res/server/main.lua", "os.<caret>"), "microtime"));
        assert!(labels("res/client/main.lua", "os.<caret>").is_empty());
        let packer = labels("res/client/main.lua", "msgpack.new():<caret>");
        assert_eq!(packer.len(), 49);
        assert!(has(&labels("res/client/main.lua", "vector3(1, 2, 3).<caret>"), "xyz"));
        assert!(labels("res/client/main.lua", "unknown.<caret>").is_empty());
        assert!(labels("res/client/main.lua", "Citizen:<caret>").is_empty());
    }

    #[test]
    fn documentation() {
        let items = complete_with(Settings::defaults(), "res/client/main.lua", "Citizen.CreateThr<caret>");
        let create = items.iter().find(|i| i.label == "CreateThread").unwrap();
        assert_eq!(create.detail.as_deref(), Some("Citizen.CreateThread(routine: fun()): integer"));
        assert_eq!(create.kind, Some(CompletionKind::Method));
        let doc = create.documentation.as_deref().unwrap();
        assert!(
            doc.starts_with(
                "```lua\nfunction Citizen.CreateThread(routine: fun()): integer\n```\n\nCreates a new scheduler thread"
            ),
            "{doc}"
        );
        assert!(doc.contains("**Parameters**\n\n- `routine` `fun()`: The function to run in the new thread."), "{doc}");
        assert!(doc.contains("**Returns**\n\n- `integer`: The scheduler bookmark id of the new thread."), "{doc}");
        assert!(doc.ends_with("*Cfx runtime API (shared)*"), "{doc}");
        let source = complete_with(Settings::defaults(), "res/server/main.lua", "sou<caret>");
        assert_eq!(source[0].detail.as_deref(), Some("source: integer"));
        let state = complete_with(Settings::defaults(), "res/client/main.lua", "LocalPlayer.<caret>");
        assert_eq!(state[0].detail.as_deref(), Some("CfxPlayer.state: StateBag"));
        let rt = runtime();
        let old = rt.decl(rt.global("RegisterServerEvent").unwrap().decl);
        assert!(markdown(rt, old).contains("**Deprecated**: Use RegisterNetEvent."));
        assert_eq!(signature(rt, rt.decl(rt.global("Citizen").unwrap().decl)), "class Citizen");
    }
}
