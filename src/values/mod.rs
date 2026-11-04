pub mod decode;
pub mod sites;
pub mod tables;

use crate::feature::{Ctx, Feature, Fix, Intention, Problem};
use crate::lua::Source;
use crate::lua::joaat::joaat;
use crate::lua::lexer::Tok;
use crate::perf::ast::{self, ExprKind, Node};
use crate::project::{FileKind, Game};
use crate::workspace::Workspace;
use decode::{Decoded, HashName, Origin};
use openidea_plugin::Severity;
use openidea_plugin::providers::{CompletionItem, CompletionKind};
use sites::{Kind, Scope, Site};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use tables::Tables;

pub mod keys {
    pub const HINTS: &str = "values.hints";
    pub const CONTROLS: &str = "values.controls";
    pub const HASHES: &str = "values.hashes";
    pub const BLIPS: &str = "values.blips";
    pub const MARKERS: &str = "values.markers";
    pub const HUD_COLOURS: &str = "values.hudColours";
    pub const EXPLOSIONS: &str = "values.explosions";
    pub const ANIM_FLAGS: &str = "values.animFlags";
}

pub const CODE: &str = "CfxMagicValue";

#[derive(Default)]
struct HashIndex {
    files: HashMap<String, Vec<(i32, String)>>,
    names: HashMap<i32, BTreeMap<String, u32>>,
}

impl HashIndex {
    fn set(&mut self, path: &str, names: Vec<(i32, String)>) {
        self.remove(path);
        for (hash, name) in &names {
            *self.names.entry(*hash).or_default().entry(name.clone()).or_default() += 1;
        }
        self.files.insert(path.to_owned(), names);
    }

    fn remove(&mut self, path: &str) {
        for (hash, name) in self.files.remove(path).unwrap_or_default() {
            if let Some(spellings) = self.names.get_mut(&hash) {
                if let Some(n) = spellings.get_mut(&name) {
                    *n -= 1;
                    if *n == 0 {
                        spellings.remove(&name);
                    }
                }
                if spellings.is_empty() {
                    self.names.remove(&hash);
                }
            }
        }
    }

    fn find(&self, hash: i32) -> Option<&str> {
        self.names.get(&hash)?.keys().next().map(String::as_str)
    }
}

fn names_of(src: &Source) -> Vec<(i32, String)> {
    let mut seen = HashMap::new();
    for t in &src.toks {
        let name = match t.kind {
            Tok::Str { closed: true } if !t.text(&src.text).contains('\\') => t.string_value(&src.text),
            Tok::LongStr => t.string_value(&src.text),
            Tok::Hash { closed: true } => {
                let (a, b) = t.content_range(&src.text);
                match unescape(&src.text[a..b]).and_then(|bytes| String::from_utf8(bytes).ok()) {
                    Some(s) => s,
                    None => continue,
                }
            }
            _ => continue,
        };
        if is_game_name(&name) {
            seen.entry(joaat(&name)).or_insert(name);
        }
    }
    seen.into_iter().collect()
}

fn is_game_name(name: &str) -> bool {
    (2..=64).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
        && name.bytes().any(|b| b.is_ascii_alphabetic())
}

fn unescape(raw: &str) -> Option<Vec<u8>> {
    let b = raw.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\r' | b'\n' => return None,
            b'\\' => {
                i += 1;
                let c = *b.get(i)?;
                i += 1;
                match c {
                    b'a' => out.push(7),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'n' | b'\n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'v' => out.push(11),
                    b'\\' | b'"' | b'\'' => out.push(c),
                    b'\r' => {
                        out.push(b'\n');
                        i += usize::from(b.get(i) == Some(&b'\n'));
                    }
                    b'z' => {
                        while b.get(i).is_some_and(u8::is_ascii_whitespace) {
                            i += 1;
                        }
                    }
                    b'x' => {
                        let hex = raw.get(i..i + 2)?;
                        out.push(u8::from_str_radix(hex, 16).ok()?);
                        i += 2;
                    }
                    b'u' => {
                        let rest = raw.get(i..)?.strip_prefix('{')?;
                        let close = rest.find('}')?;
                        let code = u32::from_str_radix(&rest[..close], 16).ok().filter(|&c| c <= 0x7FFF_FFFF)?;
                        out.extend(utf8_extended(code));
                        i += close + 2;
                    }
                    b'0'..=b'9' => {
                        let mut v = u32::from(c - b'0');
                        for _ in 0..2 {
                            match b.get(i) {
                                Some(d) if d.is_ascii_digit() => {
                                    v = v * 10 + u32::from(d - b'0');
                                    i += 1;
                                }
                                _ => break,
                            }
                        }
                        out.push(u8::try_from(v).ok()?);
                    }
                    _ => return None,
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Some(out)
}

fn utf8_extended(code: u32) -> Vec<u8> {
    if code < 0x80 {
        return vec![code as u8];
    }
    let mut bytes = Vec::new();
    let mut c = code;
    let mut limit = 0x3f;
    while c > limit {
        bytes.push(0x80 | (c & 0x3f) as u8);
        c >>= 6;
        limit >>= 1;
    }
    bytes.push(((!limit << 1) as u8) | c as u8);
    bytes.reverse();
    bytes
}

fn literal_safe(name: &str) -> bool {
    !name.is_empty() && !name.contains(['`', '\\', '\n', '\r'])
}

#[derive(Default)]
pub struct ValuesFeature {
    hashes: RefCell<HashIndex>,
}

impl ValuesFeature {
    fn applies(cx: &Ctx) -> bool {
        cx.settings.enabled() && cx.doc.is_lua()
    }

    fn hash_name(&self, hash: i32) -> Option<HashName> {
        if let Some(n) = self.hashes.borrow().find(hash) {
            return Some(HashName { name: n.to_owned(), origin: Origin::Project });
        }
        Tables::get()
            .hash_name(hash)
            .map(|(n, c)| HashName { name: n.to_owned(), origin: Origin::Bundled(c.to_owned()) })
    }

    fn decode(&self, site: &Site) -> Option<Decoded> {
        decode::decode(site.kind, site.value, site.game, &|h| self.hash_name(h))
    }

    fn site_near<'a>(scope: &Scope<'a>, at: usize) -> Option<Site<'a>> {
        let src = scope.src();
        [Some(at), at.checked_sub(1)].into_iter().flatten().filter_map(|a| src.tok_at(a)).find_map(|tok| {
            match scope.ast.owner_of(tok) {
                Some(Node::Expr(e)) => scope.site_at(e),
                _ => None,
            }
        })
    }
}

fn is_annotated(src: &Source, end: usize, name: &str) -> bool {
    let i = src.all.partition_point(|t| t.start < end);
    src.all.get(i).is_some_and(|t| {
        let gap = &src.text[end..t.start];
        t.is_comment()
            && gap.chars().all(char::is_whitespace)
            && !gap.contains('\n')
            && t.text(&src.text).contains(name)
    })
}

impl Feature for ValuesFeature {
    fn index_file(&self, _ws: &Workspace, path: &str, kind: FileKind, src: &Source) {
        let mut index = self.hashes.borrow_mut();
        if matches!(kind, FileKind::Lua | FileKind::Manifest) {
            index.set(path, names_of(src));
        } else {
            index.remove(path);
        }
    }

    fn forget_file(&self, path: &str) {
        self.hashes.borrow_mut().remove(path);
    }

    fn problems(&self, cx: &Ctx) -> Vec<Problem> {
        if !Self::applies(cx) || !cx.settings.bool(keys::HINTS, true) {
            return Vec::new();
        }
        let tree = ast::parse(&cx.doc.src);
        let scope = Scope::new(cx, &tree);
        let mut out = Vec::new();
        let mut enabled: HashMap<&str, bool> = HashMap::new();
        for (e, expr) in tree.exprs.iter().enumerate() {
            if !matches!(expr.kind, ExprKind::Number) {
                continue;
            }
            let Some(site) = scope.site_at(e) else { continue };
            let key = site.kind.setting();
            if !*enabled.entry(key).or_insert_with(|| cx.settings.bool(key, true)) {
                continue;
            }
            let Some(decoded) = self.decode(&site) else { continue };
            let (start, end) = tree.span(Node::Expr(site.expr));
            if is_annotated(&cx.doc.src, end, &decoded.name) {
                continue;
            }
            out.push(Problem::new(CODE, Severity::Hint, start, end, decoded.message()));
        }
        out
    }

    fn completion(&self, cx: &Ctx, at: usize, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if !Self::applies(cx) {
            return Vec::new();
        }
        let tree = ast::parse(&cx.doc.src);
        let scope = Scope::new(cx, &tree);
        let Some(tok) = cx.doc.src.tok_before(at) else { return Vec::new() };
        let mut n = tree.owner_of(tok);
        while let Some(node) = n {
            if let Node::Expr(call) = node
                && matches!(tree.exprs[call].kind, ExprKind::Call { .. })
                && let Some(i) = tree.call_args(call).iter().position(|&a| {
                    let arg = &tree.exprs[a];
                    arg.start <= at && at <= arg.stop && matches!(arg.kind, ExprKind::Name { .. } | ExprKind::Error)
                })
            {
                let Some(vc) = scope.call_at(call) else { return Vec::new() };
                let Some(kind) = vc.roles.kind(i) else { return Vec::new() };
                let arg = &tree.exprs[tree.call_args(call)[i]];
                let replace = matches!(arg.kind, ExprKind::Name { .. }).then(|| cx.doc.range(arg.start, arg.stop));
                let argument = decode::argument(i, vc.param(i), &vc.name);
                return items(kind, vc.game, &argument, replace);
            }
            if matches!(node, Node::Stat(_)) {
                break;
            }
            n = tree.parent(node);
        }
        Vec::new()
    }

    fn intentions(&self, cx: &Ctx, start: usize, _end: usize) -> Vec<Intention> {
        if !Self::applies(cx) {
            return Vec::new();
        }
        let tree = ast::parse(&cx.doc.src);
        let scope = Scope::new(cx, &tree);
        let Some(site) = Self::site_near(&scope, start) else { return Vec::new() };
        let Some(decoded) = self.decode(&site) else { return Vec::new() };
        let (from, to) = tree.span(Node::Expr(site.expr));
        let mut out = Vec::new();
        if !is_annotated(&cx.doc.src, to, &decoded.name) {
            let comment = decoded.comment();
            out.push(Intention {
                fix: Fix::new(format!("Add '{comment}' comment")).insert(to, format!(" {comment}")),
                kind: "refactor",
            });
        }
        let name = decoded.hash_name.as_ref().map_or(&decoded.name, |h| &h.name);
        if !(0..=0xFFFF).contains(&site.value) && literal_safe(name) && joaat(name) == site.value as i32 {
            out.push(Intention {
                fix: Fix::new(format!("Replace with `{name}`")).replace(from, to, format!("`{name}`")),
                kind: "refactor",
            });
        }
        out
    }
}

fn items(kind: Kind, game: Game, argument: &str, replace: Option<openidea_plugin::Range>) -> Vec<CompletionItem> {
    let t = Tables::get();
    let gta = game == Game::Gta5;
    let values: Vec<(i64, &str, Option<String>)> = match kind {
        Kind::Control if gta => {
            t.controls.iter().map(|c| (c.id, c.name.as_str(), Some(c.keys()).filter(|k| !k.is_empty()))).collect()
        }
        Kind::BlipSprite if gta => t.blip_sprites.iter().map(|b| (b.id, b.name.as_str(), None)).collect(),
        Kind::BlipColour if gta => {
            t.blip_colours.iter().map(|c| (c.id, c.name.as_str(), Some(c.hex.clone()))).collect()
        }
        Kind::MarkerType if gta => t.markers.iter().map(|m| (m.id, m.name.as_str(), None)).collect(),
        Kind::HudColour if gta => t
            .hud_colours
            .iter()
            .map(|h| {
                let [r, g, b, a] = h.rgba;
                (h.id, h.name.as_str(), Some(format!("{r}, {g}, {b}, {a}")))
            })
            .collect(),
        Kind::ExplosionType => {
            let table = if game == Game::Rdr3 { &t.explosions_rdr3 } else { &t.explosions_gta5 };
            table.iter().map(|e| (e.id, e.name.as_str(), None)).collect()
        }
        _ => Vec::new(),
    };
    values
        .into_iter()
        .map(|(id, name, ty)| {
            let mut item = CompletionItem::new(name, CompletionKind::Constant);
            item.detail = Some(match ty {
                Some(ty) => format!("= {id} ({ty})"),
                None => format!("= {id}"),
            });
            item.insert_text = id.to_string();
            item.replace = replace;
            item.sort_text = Some(format!("{id:05}"));
            item.documentation =
                decode::decode(kind, id, game, &|_| None).map(|d| d.markdown(&id.to_string(), Some(argument)));
            item
        })
        .collect()
}

#[cfg(test)]
mod tests;
