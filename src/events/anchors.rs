use crate::events::flow::{self, Arg};
use crate::events::model::{EventFn, Seg, is_standard_name, standard};
use crate::lua::Source;
use crate::nui::NUI_EVENT_PREFIX;
use crate::refactor::edges::{Anchor, Family, in_order, literal_name};

fn named_params(function: &EventFn) -> impl Iterator<Item = usize> + '_ {
    function.key.components.iter().filter_map(|component| match component.as_slice() {
        [Seg::Param(index)] => Some(*index),
        _ => None,
    })
}

/// The event names a file writes to the standard event functions of the runtime, one anchor each.
pub fn anchors(src: &Source) -> Vec<Anchor> {
    if !src.text.contains("Event") {
        return Vec::new();
    }
    let functions = standard();
    let mut out = Vec::new();
    for call in flow::facts(src).calls.of(is_standard_name) {
        if !call.original && (call.colon || call.callee.contains('.')) {
            continue;
        }
        let Some(function) = functions.iter().find(|function| function.path == call.callee) else { continue };
        for index in named_params(function) {
            let Some(Arg::Str { lit: Some(span), .. }) = call.args.get(index) else { continue };
            let Some((start, end)) = literal_name(src, span.start as usize) else { continue };
            let name = &src.text[start..end];
            if !name.starts_with(NUI_EVENT_PREFIX) {
                out.push(Anchor { family: Family::Cfx, key: format!("cfx.event#{name}"), start, end });
            }
        }
    }
    in_order(out)
}

#[cfg(test)]
#[path = "anchors_tests.rs"]
mod tests;
