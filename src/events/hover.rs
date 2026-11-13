use crate::docs::target::{self, Target};
use crate::editor::hover::Hover;
use crate::events::EventsFeature;
use crate::events::editor::{builtin_doc, key_doc, site_at_caret};
use crate::feature::Ctx;

/// The page of the event named by the string under `at` or ending there: the catalog page of a built-in event,
/// which never takes the page of the project, else the page of the project key.
pub fn hover(events: &EventsFeature, cx: &Ctx, at: usize) -> Option<Hover> {
    if !cx.settings.enabled() || !cx.doc.is_lua() {
        return None;
    }
    let src = &cx.doc.src;
    // `tok_at(x)` holds the byte before `x`: the token under `at`, then the one that ends there.
    let tok = [at + 1, at].into_iter().filter_map(|x| src.tok_at(x)).map(|i| src.toks[i]).find(|t| t.is_string())?;
    let range = (tok.start, tok.end);
    let inside = tok.start + 1;
    if let Some(Target::Event(event)) = target::find(cx.doc, inside, cx.natives, &[]) {
        return Some(Hover { range, markdown: builtin_doc(event) });
    }
    let db = events.db(cx)?;
    let (site, _) = site_at_caret(cx, &db, inside)?;
    if site.key.is_builtin() {
        return None;
    }
    Some(Hover { range, markdown: key_doc(cx, &db, &site.key) })
}

#[cfg(test)]
#[path = "hover_tests.rs"]
mod tests;
