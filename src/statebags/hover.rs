use crate::editor::hover::Hover;
use crate::feature::Ctx;
use crate::statebags::sites::{analyze, may_hold_bags};
use crate::statebags::{StateBagsFeature, key_doc, site_at};

/// The page of the state bag key written at `at`, as a field name or as a string.
pub fn hover(bags: &StateBagsFeature, cx: &Ctx, at: usize) -> Option<Hover> {
    if !may_hold_bags(cx.doc.text()) {
        return None;
    }
    bags.with_project(cx, |project| {
        let sites = analyze(&cx.doc.src);
        let site = site_at(&sites, at)?;
        let key = site.key.as_deref()?;
        Some(Hover { range: (site.start, site.end), markdown: key_doc(cx.ws, project, key, site.kind) })
    })?
}

#[cfg(test)]
#[path = "hover_tests.rs"]
mod tests;
