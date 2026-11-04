//! The bundled documentation content (`data/docs`): guides, built-in events,
//! manifest directives and CfxLua syntax features. Parsed once, on first use;
//! a malformed file gives an empty list (and a log line), never a failure.

use crate::project::{Game, Side};
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Clone, Debug, Deserialize)]
pub struct Guide {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    /// The Markdown text (LF line endings).
    #[serde(skip)]
    pub markdown: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct EventParam {
    pub name: String,
    #[serde(rename = "type", default = "any")]
    pub ty: String,
    #[serde(default)]
    pub description: String,
}

fn any() -> String {
    "any".into()
}

#[derive(Clone, Debug, Deserialize)]
pub struct BuiltinEvent {
    pub name: String,
    #[serde(default, rename = "side")]
    side_raw: String,
    #[serde(default, rename = "game")]
    game_raw: String,
    #[serde(default)]
    pub params: Vec<EventParam>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub cancelable: bool,
    #[serde(default, rename = "docUrl")]
    pub doc_url: String,
}

impl BuiltinEvent {
    /// Where the event is raised (`shared` when unknown).
    pub fn side(&self) -> Side {
        Side::parse(&self.side_raw).unwrap_or(Side::Shared)
    }

    /// `None`: every game.
    pub fn game(&self) -> Option<Game> {
        Game::from_manifest(&self.game_raw)
    }

    /// `(resourceName)`, `()`.
    pub fn params_text(&self) -> String {
        format!("({})", self.params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", "))
    }

    /// Pattern entries (`__cfx_nui:*`) are documentation only.
    pub fn is_pattern(&self) -> bool {
        self.name.contains('*')
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Directive {
    pub name: String,
    #[serde(default)]
    pub plural: bool,
    #[serde(default = "string", rename = "valueType")]
    pub value_type: String,
    #[serde(default)]
    pub values: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub example: String,
    #[serde(default)]
    pub deprecated: bool,
    #[serde(default, rename = "docUrl")]
    pub doc_url: String,
}

fn string() -> String {
    "string".into()
}

impl Directive {
    /// `glob list`, `enum`, `path|url`.
    pub fn value_type_text(&self) -> String {
        if self.plural { format!("{} list", self.value_type) } else { self.value_type.clone() }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct SyntaxFeature {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub syntax: String,
    #[serde(default)]
    pub example: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub pitfalls: Vec<String>,
}

/// Guide Markdown files, by file name.
const GUIDE_FILES: &[(&str, &str)] = &[
    ("cfxlua-std-extensions.md", include_str!("../../data/docs/guides/cfxlua-std-extensions.md")),
    ("cfxlua-syntax.md", include_str!("../../data/docs/guides/cfxlua-syntax.md")),
    ("commands-and-keymaps.md", include_str!("../../data/docs/guides/commands-and-keymaps.md")),
    ("escrow.md", include_str!("../../data/docs/guides/escrow.md")),
    ("events.md", include_str!("../../data/docs/guides/events.md")),
    ("exports.md", include_str!("../../data/docs/guides/exports.md")),
    ("fxmanifest-essentials.md", include_str!("../../data/docs/guides/fxmanifest-essentials.md")),
    ("fxmanifest.md", include_str!("../../data/docs/guides/fxmanifest.md")),
    ("getting-started.md", include_str!("../../data/docs/guides/getting-started.md")),
    ("json-and-msgpack.md", include_str!("../../data/docs/guides/json-and-msgpack.md")),
    ("natives.md", include_str!("../../data/docs/guides/natives.md")),
    ("nui.md", include_str!("../../data/docs/guides/nui.md")),
    ("performance.md", include_str!("../../data/docs/guides/performance.md")),
    ("plugin-guide.md", include_str!("../../data/docs/guides/plugin-guide.md")),
    ("redm-vs-fivem.md", include_str!("../../data/docs/guides/redm-vs-fivem.md")),
    ("security.md", include_str!("../../data/docs/guides/security.md")),
    ("state-bags.md", include_str!("../../data/docs/guides/state-bags.md")),
    ("threads-and-timing.md", include_str!("../../data/docs/guides/threads-and-timing.md")),
    ("vectors-and-math.md", include_str!("../../data/docs/guides/vectors-and-math.md")),
];

fn parse_list<T: for<'de> Deserialize<'de>>(what: &str, json: &str) -> Vec<T> {
    match serde_json::from_str(json) {
        Ok(v) => v,
        Err(e) => {
            openidea_plugin::host::log(openidea_plugin::LogLevel::Warn, &format!("{what}: {e}"));
            Vec::new()
        }
    }
}

/// The guides, in index order (display order, grouped by category).
pub fn guides() -> &'static [Guide] {
    static GUIDES: OnceLock<Vec<Guide>> = OnceLock::new();
    GUIDES.get_or_init(|| {
        let mut guides: Vec<Guide> = parse_list("guides/index.json", include_str!("../../data/docs/guides/index.json"));
        for g in &mut guides {
            if g.title.is_empty() {
                g.title = g.id.clone();
            }
            if g.file.is_empty() {
                g.file = format!("{}.md", g.id);
            }
            g.markdown = GUIDE_FILES
                .iter()
                .find(|(f, _)| *f == g.file)
                .map(|(_, md)| md.replace("\r\n", "\n").replace('\r', "\n"))
                .unwrap_or_default();
        }
        guides.retain(|g| !g.markdown.is_empty());
        guides
    })
}

pub fn guide(id: &str) -> Option<&'static Guide> {
    guides().iter().find(|g| g.id == id)
}

/// Built-in events (`onResourceStart`, `playerConnecting`…), catalog order.
pub fn events() -> &'static [BuiltinEvent] {
    static EVENTS: OnceLock<Vec<BuiltinEvent>> = OnceLock::new();
    EVENTS.get_or_init(|| parse_list("events/catalog.json", include_str!("../../data/docs/events/catalog.json")))
}

pub fn event(name: &str) -> Option<&'static BuiltinEvent> {
    events().iter().find(|e| e.name == name)
}

/// Manifest directives, documentation order.
pub fn directives() -> &'static [Directive] {
    static DIRECTIVES: OnceLock<Vec<Directive>> = OnceLock::new();
    DIRECTIVES.get_or_init(|| {
        parse_list("manifest/directives.json", include_str!("../../data/docs/manifest/directives.json"))
    })
}

pub fn directive(name: &str) -> Option<&'static Directive> {
    directives().iter().find(|d| d.name == name)
}

/// CfxLua syntax features.
pub fn syntax_features() -> &'static [SyntaxFeature] {
    static FEATURES: OnceLock<Vec<SyntaxFeature>> = OnceLock::new();
    FEATURES.get_or_init(|| parse_list("syntax/features.json", include_str!("../../data/docs/syntax/features.json")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_content_loads() {
        assert!(guides().len() >= 19, "{}", guides().len());
        assert!(guides().iter().all(|g| !g.title.is_empty() && !g.category.is_empty() && !g.markdown.contains('\r')));
        let start = event("onResourceStart").unwrap();
        assert_eq!((start.side(), start.game(), start.params_text().as_str()), (Side::Shared, None, "(resourceName)"));
        assert!(events().len() >= 50);
        assert!(event("playerConnecting").unwrap().cancelable);
        let fx = directive("fx_version").unwrap();
        assert!(fx.values.iter().any(|v| v == "cerulean"));
        assert_eq!(directive("client_script").unwrap().value_type_text(), "glob list");
        assert!(syntax_features().len() >= 12);
    }
}
