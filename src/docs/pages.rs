//! The detail pages of the tool window: native sheet, event, manifest
//! directive, syntax feature, guide and guide category (the runtime API pages
//! are in [`super::runtime_pages`]).

use super::data::{BuiltinEvent, Directive, Guide, SyntaxFeature, guides};
use super::html::{Table, badge, code_block, esc, markdown, markdown_inline, subhead};
use crate::natives::Native;

/// The detail of no selection.
pub const NOTHING_SELECTED: &str = "<p class='empty'>Select an item to see its documentation.</p>";

fn title(html: &str) -> String {
    format!("<div class='title'>{html}</div>")
}

fn links(url: &str) -> String {
    format!("<p class='links'><a href='{}'>Online documentation</a></p>", esc(url))
}

/// The Lua type, the native (C) type below it.
fn type_cell(lua: &str, native: &str) -> String {
    format!("<code class='id'>{}</code><br><span class='ctype' title='Native type'>{}</span>", esc(lua), esc(native))
}

fn id_cell(name: &str) -> String {
    format!("<code class='id'>{}</code>", esc(name))
}

fn note(text: &str) -> String {
    format!(" <span class='grayed'>{}</span>", esc(text))
}

/// The native sheet.
pub fn native(n: &Native) -> String {
    let name =
        if n.is_alias() { format!("<span class='deprecated'>{}</span>", esc(&n.lua_name)) } else { esc(&n.lua_name) };
    let mut out = title(&name);
    out.push_str("<p class='badges'>");
    out.push_str(&badge(&n.namespace, "badge"));
    out.push(' ');
    out.push_str(&badge(n.side_label(), n.apiset.id()));
    out.push(' ');
    out.push_str(&badge(n.game_label(), "accent"));
    if n.rpc {
        out.push(' ');
        out.push_str(&badge("RPC", "server"));
    }
    if n.is_alias() {
        out.push(' ');
        out.push_str(&badge("Deprecated", "warning"));
    }
    if n.is_unnamed() {
        out.push(' ');
        out.push_str(&badge("Unnamed", "warning"));
    }
    out.push_str("</p>");
    if let Some(primary) = &n.alias_of {
        let link = format!("<a href='cfxnative:{}:{}'>{}</a>", n.set.id(), esc(primary), esc(primary));
        out.push_str(&format!("<p>Deprecated alias: use {link} instead.</p>"));
    }
    out.push_str(&code_block(&n.call_statement()));
    let params: Vec<_> = n.lua_params().collect();
    if !params.is_empty() {
        out.push_str(&subhead("Parameters"));
        let mut t = Table::new(&["Name", "Type", "Description"]);
        for p in params {
            let kept =
                if p.kept_trailing_pointer { note("(pointer kept in the call, also returned)") } else { String::new() };
            t.row(vec![
                id_cell(&p.name),
                type_cell(&p.lua_type, &p.native_type),
                esc(p.description.as_deref().unwrap_or("")) + &kept,
            ]);
        }
        out.push_str(&t.html());
    }
    if !n.returns.is_empty() {
        out.push_str(&subhead("Returns"));
        let mut t = Table::new(&["Name", "Type", "Description"]);
        for r in &n.returns {
            let out_pointer = n.params.iter().any(|p| p.out_pointer && p.name == r.name);
            let kept = if out_pointer { note("(out-pointer: returned, not passed)") } else { String::new() };
            t.row(vec![
                id_cell(&r.name),
                type_cell(&r.lua_type, &r.native_type),
                esc(r.description.as_deref().unwrap_or("")) + &kept,
            ]);
        }
        out.push_str(&t.html());
    }
    if !n.description.trim().is_empty() {
        out.push_str(&subhead("Description"));
        out.push_str(&markdown(&n.description));
    }
    if let Some(example) = n.lua_example.as_deref().filter(|e| !e.trim().is_empty()) {
        out.push_str(&subhead("Example"));
        out.push_str(&code_block(example));
    }
    out.push_str(&subhead("Details"));
    let mut t = Table::default();
    let original =
        n.original_name.as_deref().map_or_else(|| esc("Unnamed (hash only)"), |o| format!("<code>{}</code>", esc(o)));
    t.row(vec![esc("Native name"), original]);
    t.row(vec![esc("Hash"), format!("<code>{}</code>", esc(&n.hash_display()))]);
    t.row(vec![esc("Namespace"), format!("<code>{}</code>", esc(&n.namespace))]);
    t.row(vec![esc("Natives set"), esc(n.set.display_name())]);
    if !n.aliases.is_empty() {
        let aliases: Vec<String> =
            n.aliases.iter().map(|a| format!("<code class='deprecated'>{}</code>", esc(a))).collect();
        t.row(vec![esc("Deprecated aliases"), aliases.join(", ")]);
    }
    if let Some(alt) = &n.alt_name {
        t.row(vec![esc("NativeDB name"), format!("<code>{}</code>", esc(alt))]);
    }
    out.push_str(&t.html());
    if !n.doc_url.is_empty() {
        out.push_str(&links(&n.doc_url));
    }
    out
}

/// `AddEventHandler('name', function(p1, p2)\nend)`.
fn handler_code(e: &BuiltinEvent) -> String {
    let params: Vec<&str> = e.params.iter().map(|p| p.name.as_str()).collect();
    format!("AddEventHandler('{}', function({})\nend)", e.name, params.join(", "))
}

/// `All games`, `FiveM`, `RedM`.
fn event_game(e: &BuiltinEvent) -> &'static str {
    e.game().map_or("All games", |g| g.label())
}

pub fn event(e: &BuiltinEvent) -> String {
    let mut out = title(&esc(&e.name));
    out.push_str("<p class='badges'>");
    out.push_str(&badge(e.side().label(), e.side().id()));
    out.push(' ');
    out.push_str(&badge(event_game(e), "accent"));
    if e.cancelable {
        out.push(' ');
        out.push_str(&badge("Cancelable", "warning"));
    }
    out.push_str("</p>");
    out.push_str(&code_block(&handler_code(e)));
    if !e.params.is_empty() {
        out.push_str(&subhead("Handler parameters"));
        let mut t = Table::new(&["Name", "Type", "Description"]);
        for p in &e.params {
            t.row(vec![id_cell(&p.name), id_cell(&p.ty), markdown_inline(&p.description)]);
        }
        out.push_str(&t.html());
    }
    if !e.description.trim().is_empty() {
        out.push_str(&subhead("Description"));
        out.push_str(&markdown(&e.description));
    }
    if e.cancelable {
        out.push_str("<p class='grayed'>Call CancelEvent() in a handler to cancel the default behavior.</p>");
    }
    if !e.doc_url.is_empty() {
        out.push_str(&links(&e.doc_url));
    }
    out
}

pub fn directive(d: &Directive) -> String {
    let name = if d.deprecated { format!("<span class='deprecated'>{}</span>", esc(&d.name)) } else { esc(&d.name) };
    let mut out = title(&name);
    out.push_str("<p class='badges'>");
    out.push_str(&badge(&d.value_type_text(), "badge"));
    if d.deprecated {
        out.push(' ');
        out.push_str(&badge("Deprecated", "warning"));
    }
    out.push_str("</p>");
    if !d.example.trim().is_empty() {
        out.push_str(&code_block(&d.example));
    }
    if !d.values.is_empty() {
        out.push_str(&subhead("Allowed values"));
        out.push_str("<ul>");
        for v in &d.values {
            out.push_str(&format!("<li><code>{}</code></li>", esc(v)));
        }
        out.push_str("</ul>");
    }
    if !d.description.trim().is_empty() {
        out.push_str(&subhead("Description"));
        out.push_str(&markdown(&d.description));
    }
    if !d.doc_url.is_empty() {
        out.push_str(&links(&d.doc_url));
    }
    out
}

pub fn syntax(f: &SyntaxFeature) -> String {
    let mut out = title(&esc(&f.title));
    if !f.syntax.trim().is_empty() {
        out.push_str(&subhead("Syntax"));
        let alternatives: Vec<String> = f.syntax.split(" | ").map(|s| format!("<code>{}</code>", esc(s))).collect();
        out.push_str(&format!("<p>{}</p>", alternatives.join(" &nbsp;|&nbsp; ")));
    }
    if !f.description.trim().is_empty() {
        out.push_str(&subhead("Description"));
        out.push_str(&markdown(&f.description));
    }
    if !f.example.trim().is_empty() {
        out.push_str(&subhead("Example"));
        out.push_str(&code_block(&f.example));
    }
    if !f.pitfalls.is_empty() {
        out.push_str(&subhead("Pitfalls"));
        out.push_str("<ul>");
        for p in &f.pitfalls {
            out.push_str(&format!("<li>{}</li>", markdown_inline(p)));
        }
        out.push_str("</ul>");
    }
    out.push_str("<p class='links'><a href='cfxdoc:cfxlua-syntax'>Read the CfxLua syntax guide</a></p>");
    out
}

pub fn guide(g: &Guide) -> String {
    format!("<p class='badges'>{}</p>{}", badge(&g.category, "accent"), markdown(&g.markdown))
}

/// A guide category: its guides with their summaries.
pub fn category(name: &str) -> String {
    let mut out = title(&esc(name));
    for g in guides().iter().filter(|g| g.category == name) {
        out.push_str(&format!(
            "<h3><a href='cfxdoc:{}'>{}</a></h3><p>{}</p>",
            esc(&g.id),
            esc(&g.title),
            esc(&g.summary)
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docs::data;
    use crate::docs::search::tests::fixture;

    fn fixture_native(name: &str) -> Native {
        fixture().into_iter().find(|n| n.lua_name == name).unwrap()
    }

    #[test]
    fn native_sheet() {
        let html = native(&fixture_native("GetGroundZFor_3dCoord"));
        let head = "<div class='title'>GetGroundZFor_3dCoord</div><p class='badges'><span class='badge'>MISC</span> \
                    <span class='client'>Client</span> <span class='accent'>FiveM</span></p>";
        assert!(html.starts_with(head), "{html}");
        for part in [
            "GET_GROUND_Z_FOR_3D_COORD",
            "0xC906A7DAB05C8D2B",
            "float*",
            "Ignore water surfaces",
            "Whether the ground was found",
            "(out-pointer: returned, not passed)",
            "<strong>Note</strong>",
            "FiveM client natives (GTA V)",
            "<div class='subhead'>Example</div><pre>",
            "<p class='links'><a href='https://docs.fivem.net/natives/?_0xC906A7DAB05C8D2B'>Online documentation</a></p>",
        ] {
            assert!(html.contains(part), "{part} in {html}");
        }
        assert_eq!(
            html.matches("<code class='id'>groundZ</code>").count(),
            1,
            "an out-pointer is a return, not a parameter"
        );
        let alias = native(&fixture_native("GetEntityCoordinates"));
        assert!(alias.contains("<span class='deprecated'>GetEntityCoordinates</span>"));
        assert!(alias.contains(
            "<p>Deprecated alias: use <a href='cfxnative:GTA5:GetEntityCoords'>GetEntityCoords</a> instead.</p>"
        ));
        assert!(alias.contains("<span class='warning'>Deprecated</span>"));
        let unnamed = native(&fixture_native("N_0x0b40ed49d7d6ff84"));
        assert!(unnamed.contains("<span class='warning'>Unnamed</span>") && unnamed.contains("Unnamed (hash only)"));
        let primary = native(&fixture_native("GetEntityCoords"));
        assert!(primary.contains("<code class='deprecated'>GetEntityCoordinates</code>"));
    }

    #[test]
    fn event_directive_syntax_and_guide_pages() {
        let start = event(data::event("onResourceStart").unwrap());
        assert!(
            start.contains("<span class='shared'>Shared</span> <span class='accent'>All games</span></p>"),
            "{start}"
        );
        assert!(start.contains("Handler parameters") && start.contains("<code class='id'>resourceName</code>"));
        let connecting = event(data::event("playerConnecting").unwrap());
        assert!(connecting.contains("<span class='warning'>Cancelable</span>"));
        assert!(connecting.contains("Call CancelEvent() in a handler to cancel the default behavior."));
        let fx = directive(data::directive("fx_version").unwrap());
        assert!(
            fx.contains("<div class='subhead'>Allowed values</div><ul>")
                && fx.contains("<li><code>cerulean</code></li>")
        );
        let lua54 = directive(data::directive("lua54").unwrap());
        assert!(
            lua54.contains("<span class='deprecated'>lua54</span>")
                && lua54.contains("<span class='warning'>Deprecated</span>")
        );
        let feature = data::syntax_features().iter().find(|f| f.id == "compound-assignment").unwrap();
        let html = syntax(feature);
        assert!(html.contains("<p><code>target += expr</code> &nbsp;|&nbsp; <code>-=</code>"), "{html}");
        assert!(html.ends_with("<a href='cfxdoc:cfxlua-syntax'>Read the CfxLua syntax guide</a></p>"));
        let natives = guide(data::guide("natives").unwrap());
        assert!(natives.starts_with("<p class='badges'><span class='accent'>Natives</span></p><h1>"), "{natives}");
        let cat = category("Natives");
        assert!(cat.contains("<h3><a href='cfxdoc:natives'>Calling natives from Lua</a></h3><p>"), "{cat}");
        assert_eq!(
            handler_code(data::event("onResourceStart").unwrap()),
            "AddEventHandler('onResourceStart', function(resourceName)\nend)"
        );
    }
}
