//! What `show-docs` documents at the caret: a native, a backtick hash
//! literal, a built-in event name (the first string argument of an event
//! function) or a manifest directive; and the plain-text summary of its
//! notification.

use super::data::{self, BuiltinEvent, Directive};
use super::tabs::Tab;
use crate::feature::Doc;
use crate::lua::joaat::{hex, joaat};
use crate::lua::lexer::Tok;
use crate::natives::{Native, NativeSet, Natives};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

/// Functions whose first argument is an event name (docs §10.1).
pub const EVENT_FUNCTIONS: [&str; 8] = [
    "AddEventHandler",
    "RegisterNetEvent",
    "TriggerEvent",
    "RegisterServerEvent",
    "TriggerServerEvent",
    "TriggerLatentServerEvent",
    "TriggerClientEvent",
    "TriggerLatentClientEvent",
];

pub enum Target<'a> {
    Native(&'a Native),
    /// The literal as written and its value (`None`: invalid).
    Hash(String, Option<i32>),
    Event(&'static BuiltinEvent),
    Directive(&'static Directive),
}

/// The target at byte `at` of `doc`; natives are looked up in `sets`, then in every set.
pub fn find<'a>(doc: &Doc, at: usize, natives: &'a Natives, sets: &[NativeSet]) -> Option<Target<'a>> {
    let src = &doc.src;
    if doc.is_manifest() {
        let entries = crate::project::manifest::entries(src);
        let entry = entries.iter().find(|e| e.name_start <= at && at <= e.end)?;
        return directive(&entry.name).map(Target::Directive);
    }
    let i = src.tok_at(at)?;
    let tok = src.toks[i];
    match tok.kind {
        Tok::Hash { closed } => {
            let (s, e) = tok.content_range(&src.text);
            let value = if closed { hash_value(&src.text[s..e]) } else { None };
            Some(Target::Hash(tok.text(&src.text).to_owned(), value))
        }
        Tok::Str { .. } => {
            let calls = src.calls(&src.chains());
            let first_arg = |c: &crate::lua::Call| c.args.first().and_then(|a| src.string_arg(*a));
            let call = calls.iter().find(|c| EVENT_FUNCTIONS.contains(&c.callee.as_str()) && first_arg(c) == Some(i))?;
            let event = data::event(&src.string_at(src.string_arg(call.args[0])?)?)?;
            (!event.is_pattern()).then_some(Target::Event(event))
        }
        Tok::Name => {
            let member = i > 0 && matches!(src.tok_text(i - 1), "." | ":" | "?." | "?:") || i > 0 && src.is_kw(i - 1, "function");
            if member {
                return None;
            }
            let name = src.tok_text(i);
            natives.find(name, sets).or_else(|| natives.find(name, &NativeSet::ALL)).map(Target::Native)
        }
        _ => None,
    }
}

/// A directive by name, plural forms included (`client_scripts` → `client_script`).
pub fn directive(name: &str) -> Option<&'static Directive> {
    data::directive(name).or_else(|| name.strip_suffix('s').and_then(data::directive))
}

/// The joaat of a hash literal's content, Lua escapes decoded; `None` when an escape is invalid.
pub fn hash_value(content: &str) -> Option<i32> {
    if !content.contains('\\') {
        return Some(joaat(content));
    }
    let mut out: Vec<u8> = Vec::new();
    let mut chars = content.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            let mut buf = [0; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            continue;
        }
        match chars.next()? {
            'n' => out.push(b'\n'),
            't' => out.push(b'\t'),
            'r' => out.push(b'\r'),
            'a' => out.push(7),
            'b' => out.push(8),
            'f' => out.push(12),
            'v' => out.push(11),
            c @ ('\\' | '\'' | '"' | '`') => out.push(c as u8),
            'z' => {
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
            }
            'x' => {
                let digits: String = (0..2).filter_map(|_| chars.next()).collect();
                out.push(u8::from_str_radix(&digits, 16).ok().filter(|_| digits.len() == 2)?);
            }
            d if d.is_ascii_digit() => {
                let mut value = d.to_digit(10)?;
                for _ in 0..2 {
                    match chars.peek().and_then(|c| c.to_digit(10)) {
                        Some(n) => {
                            value = value * 10 + n;
                            chars.next();
                        }
                        None => break,
                    }
                }
                out.push(u8::try_from(value).ok()?);
            }
            _ => return None,
        }
    }
    String::from_utf8(out).ok().map(|s| joaat(&s))
}

/// The first sentence of a Markdown text, as plain text.
pub fn first_sentence(md: &str) -> String {
    let mut out = String::new();
    let mut skip = false;
    for event in Parser::new(md) {
        match event {
            Event::Start(Tag::CodeBlock(_)) => skip = true,
            Event::End(TagEnd::CodeBlock) => skip = false,
            Event::Text(t) | Event::Code(t) if !skip => out.push_str(&t),
            Event::SoftBreak | Event::HardBreak => out.push(' '),
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item) if !out.trim().is_empty() => break,
            _ => {}
        }
        if let Some(end) = out.find(". ") {
            out.truncate(end + 1);
            break;
        }
    }
    out.trim().to_owned()
}

/// The page to open (tab, item id) and the notification (title, body) of a target.
pub fn summary(t: &Target) -> (Tab, String, String, String) {
    let footer = "See the Sensity tool window.";
    let lines = |head: String, md: &str| {
        let sentence = first_sentence(md);
        let mut out = head;
        if !sentence.is_empty() {
            out.push('\n');
            out.push_str(&sentence);
        }
        out.push('\n');
        out.push_str(footer);
        out
    };
    match t {
        Target::Native(n) => (Tab::Natives, n.key(), n.lua_name.clone(), lines(n.typed_signature(), &n.description)),
        Target::Event(e) => (Tab::Events, e.name.clone(), e.name.clone(), lines(format!("event {}{}", e.name, e.params_text()), &e.description)),
        Target::Directive(d) => {
            (Tab::Manifest, d.name.clone(), d.name.clone(), lines(format!("{} ({})", d.name, d.value_type_text()), &d.description))
        }
        Target::Hash(literal, value) => {
            let head = match value {
                Some(v) => format!("Signed (int32): {v} · Unsigned (uint32): {} · Hex: {}", *v as u32, hex(*v)),
                None => "Invalid hash literal: it is unterminated or has an escape sequence the runtime rejects.".into(),
            };
            let md = "Compile-time Jenkins one-at-a-time hash (joaat) of the ASCII lower-cased text, replaced by a signed 32-bit integer.";
            (Tab::Syntax, "hash-literal".into(), format!("hash {literal}"), lines(head, md))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docs::search::tests::fixture;
    use crate::testing::Fixture;

    fn target_at(rel: &str, code: &str) -> Option<(Tab, String, String, String)> {
        let fx = Fixture::new(&[("res/fxmanifest.lua", "game 'gta5'\nclient_script 'client/*.lua'"), (rel, code)]).with_natives(fixture());
        let (doc, at) = fx.caret(rel);
        find(&doc, at, &fx.natives, &[NativeSet::Gta5]).as_ref().map(summary)
    }

    #[test]
    fn natives_events_hashes_and_directives() {
        let (tab, id, title, body) = target_at("res/client/main.lua", "local c = GetEntity<caret>Coords(PlayerPedId(), false)").unwrap();
        assert_eq!((tab, id.as_str(), title.as_str()), (Tab::Natives, "GTA5:GetEntityCoords", "GetEntityCoords"));
        assert_eq!(body, "GetEntityCoords(entity: integer, alive: boolean): vector3\nSee the Sensity tool window.");
        let (_, id, _, body) = target_at("res/client/main.lua", "GetGroundZFor_3dCoord<caret>(0, 0, 0)").unwrap();
        assert_eq!(id, "GTA5:GetGroundZFor_3dCoord");
        assert!(body.contains("\nGets the ground elevation at the specified position.\n"), "{body}");
        assert_eq!(target_at("res/client/main.lua", "DropPl<caret>ayer(1)").unwrap().1, "SERVER:DropPlayer", "any set as a fallback");
        assert!(target_at("res/client/main.lua", "x.GetEntity<caret>Coords()").is_none());
        let (tab, id, _, body) = target_at("res/client/main.lua", "AddEventHandler('onResource<caret>Start', function() end)").unwrap();
        assert_eq!((tab, id.as_str()), (Tab::Events, "onResourceStart"));
        assert!(body.starts_with("event onResourceStart(resourceName)\n"), "{body}");
        assert!(target_at("res/client/main.lua", "print('onResource<caret>Start')").is_none());
        assert!(target_at("res/client/main.lua", "TriggerEvent('x', 'onResource<caret>Start')").is_none());
        assert!(target_at("res/client/main.lua", "AddEventHandler('myres:<caret>x')").is_none());
        let (tab, id, title, body) = target_at("res/client/main.lua", "local h = `ad<caret>der`").unwrap();
        assert_eq!((tab, id.as_str(), title.as_str()), (Tab::Syntax, "hash-literal", "hash `adder`"));
        assert!(body.starts_with("Signed (int32): -1216765807 · Unsigned (uint32): 3078201489 · Hex: 0xB779A091\n"), "{body}");
        assert!(target_at("res/client/main.lua", "local h = `ad<caret>der").unwrap().3.starts_with("Invalid hash literal"));
        let fx = Fixture::new(&[("res/fxmanifest.lua", "fx_version 'cerulean'\nclient_scripts {\n  'cl<caret>ient.lua'\n}")]);
        let (doc, at) = fx.caret("res/fxmanifest.lua");
        let (tab, id, _, body) = summary(&find(&doc, at, &fx.natives, &[]).unwrap());
        assert_eq!((tab, id.as_str()), (Tab::Manifest, "client_script"));
        assert!(body.starts_with("client_script (glob list)\nScript(s) loaded on each client"), "{body}");
        assert!(target_at("res/client/main.lua", "local x<caret> = 1").is_none());
    }

    #[test]
    fn hash_values_and_sentences() {
        assert_eq!(hash_value("AdDeR"), Some(-1216765807));
        assert_eq!(hash_value("a\\x64der"), Some(joaat("adder")));
        assert_eq!(hash_value("a\\100der"), Some(joaat("adder")));
        assert_eq!(hash_value("bad\\q"), None);
        assert_eq!(hash_value("\\x6"), None);
        assert_eq!(hex(0), "0x00000000");
        assert_eq!(first_sentence("Runs `x` now. Then more."), "Runs x now.");
        assert_eq!(first_sentence("```lua\ncode()\n```\n\nThe text"), "The text");
        assert_eq!(first_sentence(""), "");
    }
}
