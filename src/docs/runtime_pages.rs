use super::html::{esc, markdown, markdown_inline};
use crate::runtime::{Decl, Part, Runtime};

pub const NOTHING_SELECTED: &str = "<p class='empty'>Select a declaration to see its documentation.</p>";
pub const NO_DOCUMENTATION: &str = "<p class='empty'>No documentation</p>";

pub fn part(rt: &Runtime, part: Part) -> String {
    let tops = &rt.tops[part as usize];
    let mut out =
        format!("<div class='title'>{}</div><p>{} top-level declarations</p><ul>", esc(part.label()), tops.len());
    for &d in tops.iter().take(200) {
        out.push_str(&format!("<li><code>{}</code></li>", esc(&rt.decl(d).label())));
    }
    out.push_str("</ul>");
    out
}

fn type_html(rt: &Runtime, ty: &str) -> String {
    let mut out = String::new();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if rt.is_class(word) {
            out.push_str(&format!("<a href=\"cfxtype:{0}\">{0}</a>", esc(word)));
        } else {
            out.push_str(&esc(word));
        }
        word.clear();
    };
    for c in ty.chars() {
        if c.is_alphanumeric() || c == '_' {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push_str(&esc(&c.to_string()));
        }
    }
    flush(&mut word, &mut out);
    out
}

fn section(title: &str, html: &str) -> String {
    format!("<tr><td valign='top' class='section'><p>{}</p></td><td valign='top'>{html}</td></tr>", esc(title))
}

fn desc(text: &str) -> String {
    if text.is_empty() { String::new() } else { format!(" - {}", markdown_inline(text)) }
}

fn definition(rt: &Runtime, d: &Decl) -> String {
    let owner = d
        .owner
        .as_deref()
        .map(|o| format!("{}{}", type_html(rt, o), if d.colon { ":" } else { "." }))
        .unwrap_or_default();
    if d.is_function() {
        let params: Vec<String> =
            d.typed_params().iter().map(|p| format!("{}: {}", esc(&p.name), type_html(rt, &p.ty))).collect();
        let mut s = format!("{owner}<b>{}</b>({})", esc(&d.name), params.join(", <br>        "));
        if !d.doc.returns.is_empty() {
            let returns: Vec<String> = d.doc.returns.iter().map(|r| type_html(rt, &r.ty)).collect();
            s.push_str(&format!(": {}", returns.join(", ")));
        }
        return s;
    }
    if let Some(class) = &d.doc.class {
        let mut s = format!("class <b>{}</b>", esc(class));
        if !d.doc.parents.is_empty() {
            let parents: Vec<String> = d.doc.parents.iter().map(|p| type_html(rt, p)).collect();
            s.push_str(&format!(" : {}", parents.join(", ")));
        }
        return s;
    }
    let ty = d
        .var_type()
        .map(str::to_owned)
        .unwrap_or_else(|| if d.children.is_empty() { "any".into() } else { "table".into() });
    format!("{owner}<b>{}</b>: {}", esc(&d.name), type_html(rt, &ty))
}

pub fn decl(rt: &Runtime, d: &Decl) -> String {
    let a = &d.doc;
    let mut out = format!("<div class='definition'><pre>{}</pre></div>", definition(rt, d));
    if !a.description.is_empty() {
        out.push_str(&format!("<div class='content'>{}</div>", markdown(&a.description)));
    }
    let mut sections = String::new();
    if let Some(msg) = &a.deprecated {
        sections.push_str(&section(
            "Deprecated:",
            &format!("<p>{}</p>", if msg.is_empty() { "Deprecated".into() } else { markdown_inline(msg) }),
        ));
    }
    let lines = |items: Vec<String>| items.into_iter().map(|i| format!("<p>{i}</p>")).collect::<String>();
    if !a.fields.is_empty() {
        let fields = a
            .fields
            .iter()
            .map(|f| format!("<code>{}</code>: {}{}", esc(&f.name), type_html(rt, &f.ty), desc(&f.desc)))
            .collect();
        sections.push_str(&section("Fields:", &lines(fields)));
    }
    let params = d.typed_params();
    if d.is_function() && !params.is_empty() {
        let params = params
            .iter()
            .map(|p| format!("<code>{}</code> : {}{}", esc(&p.name), type_html(rt, &p.ty), desc(&p.desc)))
            .collect();
        sections.push_str(&section("Parameters:", &lines(params)));
    }
    if !a.returns.is_empty() {
        let returns = a.returns.iter().map(|r| format!("{}{}", type_html(rt, &r.ty), desc(&r.desc))).collect();
        sections.push_str(&section("Returns:", &lines(returns)));
    }
    if !a.overloads.is_empty() {
        sections.push_str(&section(
            "Overloads:",
            &lines(a.overloads.iter().map(|o| format!("<code>{}</code>", esc(o))).collect()),
        ));
    }
    if !a.see.is_empty() {
        sections.push_str(&section("See:", &lines(a.see.iter().map(|s| esc(s)).collect())));
    }
    if !sections.is_empty() {
        out.push_str(&format!("<table class='sections'>{sections}</table>"));
    }
    out.push_str(&format!("<p class='grayed'>{} · {}</p>", esc(d.part.display_name()), esc(d.file)));
    out
}

pub fn class(rt: &Runtime, name: &str) -> String {
    rt.class(name).map_or_else(|| NO_DOCUMENTATION.to_owned(), |d| decl(rt, d))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::runtime;

    #[test]
    fn declaration_pages() {
        let rt = runtime();
        let create = rt.members("Citizen").iter().map(|&m| rt.decl(m)).find(|d| d.name == "SetTimeout").unwrap();
        let html = decl(rt, create);
        let definition = "<div class='definition'><pre><a href=\"cfxtype:Citizen\">Citizen</a>.<b>SetTimeout</b>\
                          (msec: integer, <br>        callback: fun()): integer</pre></div>";
        assert!(html.starts_with(definition), "{html}");
        assert!(html.contains("<div class='content'><p>Runs <code>callback</code>"), "{html}");
        let params = "<tr><td valign='top' class='section'><p>Parameters:</p></td><td valign='top'>\
                      <p><code>msec</code> : integer - Delay in milliseconds.</p>";
        assert!(html.contains(params), "{html}");
        assert!(html.contains("<p>Returns:</p></td><td valign='top'><p>integer - Timeout id, usable with <code>ClearTimeout</code>.</p>"), "{html}");
        let entity = rt.decl(rt.global("Entity").unwrap().decl);
        let html = decl(rt, entity);
        assert!(html.contains("entity: integer|<a href=\"cfxtype:CfxEntity\">CfxEntity</a>"), "{html}");
        let player = class(rt, "CfxPlayer");
        assert!(player.starts_with("<div class='definition'><pre>class <b>CfxPlayer</b></pre></div>"), "{player}");
        assert!(
            player.contains("<p>Fields:</p>")
                && player.contains("<code>state</code>: <a href=\"cfxtype:StateBag\">StateBag</a>")
        );
        assert_eq!(class(rt, "Nope"), NO_DOCUMENTATION);
        let old = decl(rt, rt.decl(rt.global("RegisterServerEvent").unwrap().decl));
        assert!(old.contains("<p>Deprecated:</p></td><td valign='top'><p>Use RegisterNetEvent.</p>"), "{old}");
        let source = decl(rt, rt.decl(rt.global("source").unwrap().decl));
        assert!(source.starts_with("<div class='definition'><pre><b>source</b>: integer</pre></div>"), "{source}");
        assert!(source.ends_with("<p class='grayed'>Cfx runtime API (server) · server_events.lua</p>"));
        let summary = part(rt, Part::Manifest);
        assert!(
            summary.starts_with("<div class='title'>Manifest</div><p>44 top-level declarations</p><ul><li><code>"),
            "{summary}"
        );
    }
}
