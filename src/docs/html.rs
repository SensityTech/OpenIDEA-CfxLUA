//! HTML building blocks of the documentation pages: GitHub-flavoured Markdown
//! (tables, strikethrough, task lists, bare URLs) with the tool window's
//! post-processing, highlighted Lua code, badges and zebra tables. The page's
//! style sheet (`web/style.css`) gives the classes their look.

use super::data;
use crate::lua::lexer::{Tok, tokenize};
use pulldown_cmark::{CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd, html};

/// Escapes text for HTML (`& < > " '`).
pub fn esc(text: &str) -> String {
    crate::text::html_escape(text)
}

/// `<span class='{class}'>{text}</span>`: `badge`, `client`, `server`, `shared`, `warning`, `accent`.
pub fn badge(text: &str, class: &str) -> String {
    format!("<span class='{class}'>{}</span>", esc(text))
}

/// `<div class='subhead'>{title}</div>`.
pub fn subhead(title: &str) -> String {
    format!("<div class='subhead'>{}</div>", esc(title))
}

/// Markdown to HTML: `cfxdoc:<id>` code spans become links titled after the
/// guide, Lua code blocks are highlighted, other code blocks lose their
/// `<code>`, table rows are striped (`<tr class="alt">`) and raw HTML is shown
/// as text (line breaks excepted).
pub fn markdown(source: &str) -> String {
    if source.trim().is_empty() {
        return String::new();
    }
    let md = source.replace("\r\n", "\n").replace('\r', "\n");
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut events: Vec<Event> = Vec::new();
    let mut code: Option<(bool, String)> = None;
    let mut text = String::new();
    let mut links = 0usize;
    for event in Parser::new_ext(&md, options) {
        if let Some((_, body)) = code.as_mut() {
            match event {
                Event::Text(t) => body.push_str(&t),
                Event::End(TagEnd::CodeBlock) => {
                    if let Some((lua, body)) = code.take() {
                        let inner = if lua { highlight(body.trim_end()) } else { esc(body.trim_end_matches('\n')) };
                        events.push(Event::Html(format!("<pre>{inner}</pre>\n").into()));
                    }
                }
                _ => {}
            }
            continue;
        }
        if let Event::Text(t) = &event {
            text.push_str(t);
            continue;
        }
        flush_text(&mut text, links == 0, &mut events);
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                let lua = matches!(&kind, CodeBlockKind::Fenced(info) if info.split_whitespace().next() == Some("lua"));
                code = Some((lua, String::new()));
            }
            Event::Code(c) => events.push(guide_link(&c).map_or(Event::Code(c), |html| Event::Html(html.into()))),
            Event::Html(h) | Event::InlineHtml(h) => events.push(raw_html(h)),
            Event::Start(Tag::Link { .. }) => {
                links += 1;
                events.push(event);
            }
            Event::End(TagEnd::Link) => {
                links = links.saturating_sub(1);
                events.push(event);
            }
            other => events.push(other),
        }
    }
    flush_text(&mut text, links == 0, &mut events);
    let mut out = String::with_capacity(md.len() * 3 / 2);
    html::push_html(&mut out, events.into_iter());
    for tag in ["<thead>", "</thead>", "<tbody>", "</tbody>"] {
        out = out.replace(tag, "");
    }
    zebra(&out).trim().to_owned()
}

/// [`markdown`] without the wrapping `<p>` of a single paragraph.
pub fn markdown_inline(source: &str) -> String {
    let html = markdown(source);
    match html.strip_prefix("<p>").and_then(|h| h.strip_suffix("</p>")) {
        Some(inner) if !inner.contains("<p>") => inner.to_owned(),
        _ => html,
    }
}

/// A `<pre>` block of highlighted Lua.
pub fn code_block(code: &str) -> String {
    format!("<pre>{}</pre>", highlight(code.trim_end()))
}

/// Lua code as HTML, tokens in `<span class='l-…'>` (`kw`, `str`, `num`,
/// `com`, `op`): the page colours them for the IDE theme.
pub fn highlight(code: &str) -> String {
    let mut out = String::with_capacity(code.len() * 2);
    let mut last = 0;
    for t in tokenize(code) {
        out.push_str(&esc(&code[last..t.start]));
        let class = match t.kind {
            Tok::Keyword => Some("kw"),
            Tok::Str { .. } | Tok::LongStr | Tok::Hash { .. } => Some("str"),
            Tok::Number => Some("num"),
            Tok::LineComment | Tok::BlockComment | Tok::CComment => Some("com"),
            Tok::Op => Some("op"),
            Tok::Name | Tok::Unknown => None,
        };
        let text = esc(t.text(code));
        match class {
            Some(c) => out.push_str(&format!("<span class='l-{c}'>{text}</span>")),
            None => out.push_str(&text),
        }
        last = t.end;
    }
    out.push_str(&esc(&code[last..]));
    out
}

/// An HTML table (`cellspacing='0' width='100%'`), header optional, every
/// second body row striped.
#[derive(Default)]
pub struct Table {
    header: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Table {
    pub fn new(header: &[&str]) -> Self {
        Self { header: header.iter().map(|h| (*h).to_owned()).collect(), rows: Vec::new() }
    }

    /// One row of HTML cells.
    pub fn row(&mut self, cells: Vec<String>) {
        self.rows.push(cells);
    }

    pub fn html(&self) -> String {
        let mut out = String::from("<table cellspacing='0' width='100%'>");
        if !self.header.is_empty() {
            out.push_str("<tr>");
            for h in &self.header {
                out.push_str(&format!("<th>{}</th>", esc(h)));
            }
            out.push_str("</tr>");
        }
        for (i, row) in self.rows.iter().enumerate() {
            out.push_str(if i % 2 == 1 { "<tr class='alt'>" } else { "<tr>" });
            for cell in row {
                out.push_str(&format!("<td>{cell}</td>"));
            }
            out.push_str("</tr>");
        }
        out.push_str("</table>");
        out
    }
}

/// The guide of a `cfxdoc:<id>[#…]` link.
fn guide_of_link(link: &str) -> Option<&'static data::Guide> {
    let id = link.strip_prefix("cfxdoc:")?;
    data::guide(id.split('#').next().unwrap_or(id).trim())
}

/// `` `cfxdoc:natives` `` → a link titled after the guide.
fn guide_link(code: &str) -> Option<String> {
    let valid = code.strip_prefix("cfxdoc:")?.bytes().all(|b| b.is_ascii_alphanumeric() || b"_#-".contains(&b));
    let guide = guide_of_link(code).filter(|_| valid)?;
    Some(format!("<a href=\"{}\">{}</a>", esc(code), esc(&guide.title)))
}

/// Raw HTML of the source: `<br>` kept, anything else shown as text (the
/// natives database is downloaded content).
fn raw_html(html: CowStr) -> Event {
    let tag = html.trim().to_ascii_lowercase().replace(' ', "");
    if matches!(tag.as_str(), "<br>" | "<br/>") { Event::Html("<br>".into()) } else { Event::Text(html) }
}

/// Emits the pending text, with bare `http(s)://` URLs as links outside links.
fn flush_text(text: &mut String, autolink: bool, events: &mut Vec<Event>) {
    if text.is_empty() {
        return;
    }
    let t = std::mem::take(text);
    if !autolink {
        events.push(Event::Text(t.into()));
        return;
    }
    let mut rest = t.as_str();
    while let Some((start, end)) = find_url(rest) {
        if start > 0 {
            events.push(Event::Text(rest[..start].to_owned().into()));
        }
        let url = esc(&rest[start..end]);
        events.push(Event::Html(format!("<a href=\"{url}\">{url}</a>").into()));
        rest = &rest[end..];
    }
    if !rest.is_empty() {
        events.push(Event::Text(rest.to_owned().into()));
    }
}

/// The first bare URL of `text` (GFM extended autolink rules, simplified):
/// starts a word, runs to whitespace or `<`, trailing punctuation and
/// unbalanced `)` left out.
fn find_url(text: &str) -> Option<(usize, usize)> {
    let mut from = 0;
    loop {
        let at = from + ["http://", "https://"].iter().filter_map(|p| text[from..].find(p)).min()?;
        let word_start = text[..at].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        let body = text[at..].find(|c: char| c.is_whitespace() || c == '<').map_or(text.len(), |i| at + i);
        let mut end = body;
        loop {
            let s = &text[at..end];
            match s.chars().next_back() {
                Some('?' | '!' | '.' | ',' | ':' | '*' | '_' | '~' | '\'' | '"' | ';') => end -= 1,
                Some(')') if s.matches(')').count() > s.matches('(').count() => end -= 1,
                _ => break,
            }
        }
        let scheme = if text[at..].starts_with("https://") { 8 } else { 7 };
        if word_start && end > at + scheme {
            return Some((at, end));
        }
        from = at + scheme;
    }
}

/// Stripes the tables of `html`: the first row of each is the header, then
/// every second row gets `class="alt"`.
fn zebra(html: &str) -> String {
    let mut out = String::with_capacity(html.len() + 64);
    let mut rest = html;
    while let Some(start) = rest.find("<table>") {
        let end = rest[start..].find("</table>").map_or(rest.len(), |e| start + e);
        out.push_str(&rest[..start]);
        let mut row = 0;
        let mut table = &rest[start..end];
        while let Some(i) = table.find("<tr>") {
            out.push_str(&table[..i]);
            row += 1;
            out.push_str(if row > 1 && row % 2 == 1 { "<tr class=\"alt\">" } else { "<tr>" });
            table = &table[i + 4..];
        }
        out.push_str(table);
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_lose_head_and_body_and_are_striped() {
        let html = markdown("| a | b |\r\n|---|---|\r\n| 1 | 2 |\r\n| 3 | 4 |\r\n| 5 | 6 |\r\n");
        assert!(html.contains("<td>1</td>"), "{html}");
        assert!(!html.contains("<thead>") && !html.contains("<tbody>"), "{html}");
        assert_eq!(html.matches("<tr class=\"alt\">").count(), 1, "{html}");
        let alt = html.find("<tr class=\"alt\">").unwrap();
        assert!(html[alt..].starts_with("<tr class=\"alt\"><td>3</td>"), "{html}");
    }

    #[test]
    fn guide_links_and_code_blocks() {
        let html = markdown(
            "See `cfxdoc:natives` and `cfxdoc:nope`.\n\n```lua\nlocal x = 1 -- one\n```\n\n```\nplain <b>\n```\n",
        );
        assert!(html.contains("<a href=\"cfxdoc:natives\">Calling natives from Lua</a>"), "{html}");
        assert!(html.contains("<code>cfxdoc:nope</code>"), "{html}");
        assert!(
            html.contains(
                "<pre><span class='l-kw'>local</span> x <span class='l-op'>=</span> <span class='l-num'>1</span>"
            ),
            "{html}"
        );
        assert!(html.contains("<span class='l-com'>-- one</span></pre>"), "{html}");
        assert!(html.contains("<pre>plain &lt;b&gt;</pre>"), "{html}");
    }

    #[test]
    fn bare_urls_task_lists_and_raw_html() {
        let html = markdown(
            "Read https://docs.fivem.net/docs/ (or <https://x.io>).\n\n- [ ] todo\n- [x] done\n\n\
             ~~old~~ <img src=x onerror=alert(1)> a<br>b",
        );
        assert!(
            html.contains("<a href=\"https://docs.fivem.net/docs/\">https://docs.fivem.net/docs/</a> (or"),
            "{html}"
        );
        assert!(html.contains("<a href=\"https://x.io\">https://x.io</a>)"), "{html}");
        assert!(html.contains("type=\"checkbox\""), "{html}");
        assert!(html.contains("<del>old</del>"), "{html}");
        assert!(html.contains("&lt;img src=x") && html.contains("a<br>b"), "{html}");
        assert_eq!(markdown_inline("One `x`."), "One <code>x</code>.");
        assert!(markdown_inline("a\n\nb").starts_with("<p>"));
        assert_eq!(markdown("  \n"), "");
    }

    #[test]
    fn url_edges() {
        assert_eq!(find_url("x https://a.b/c_(d)."), Some((2, 19)));
        assert_eq!(find_url("(https://a.b/c)"), Some((1, 14)));
        assert_eq!(find_url("nohttps://a.b"), None);
        assert_eq!(find_url("https:// x"), None);
    }

    #[test]
    fn every_guide_renders() {
        for g in data::guides() {
            let html = markdown(&g.markdown);
            assert!(html.starts_with("<h1>"), "{}", g.id);
            assert!(!html.contains("<thead>"), "{}", g.id);
            assert!(!html.contains("<code>cfxdoc:"), "unresolved guide link in {}", g.id);
            if g.markdown.contains("\n|---") || g.markdown.contains("\n| ---") {
                assert!(html.contains("<table>"), "{}", g.id);
            }
        }
    }

    #[test]
    fn tables_and_badges() {
        let mut t = Table::new(&["Name", "Type"]);
        t.row(vec!["a".into(), "b".into()]);
        t.row(vec!["c".into(), "d".into()]);
        assert_eq!(
            t.html(),
            "<table cellspacing='0' width='100%'><tr><th>Name</th><th>Type</th></tr>\
             <tr><td>a</td><td>b</td></tr><tr class='alt'><td>c</td><td>d</td></tr></table>"
        );
        assert_eq!(badge("A<B", "accent"), "<span class='accent'>A&lt;B</span>");
        assert_eq!(highlight("`a` \"s\""), "<span class='l-str'>`a`</span> <span class='l-str'>&quot;s&quot;</span>");
    }
}
