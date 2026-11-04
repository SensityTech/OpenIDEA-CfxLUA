//! The manifest inspections: a missing or invalid `fx_version`, `game` and
//! `rdr3_warning`; deprecated content; misspelled or misused directives; paths
//! and globs matching no file of the resource.

use super::catalog::{self, FX_VERSION_LINE, FX_VERSIONS, GAMES, RDR3_WARNING, Role, rdr3_warning_line};
use super::edits::{self, Edit};
use super::files::{Tree, checkable, is_glob, is_url, split_resource_path};
use super::view::{Decl, View};
use crate::feature::{Ctx, Fix, Problem};
use crate::project::Value;
use crate::text::{edit_distance, file_name, parent};
use openidea_plugin::Severity;

pub const FX_VERSION: &str = "CfxManifestFxVersion";
pub const GAME: &str = "CfxManifestGame";
pub const RDR3: &str = "CfxManifestRdr3Warning";
pub const DEPRECATED: &str = "CfxManifestDeprecated";
pub const UNKNOWN: &str = "CfxManifestUnknownDirective";
pub const MISSING_FILE: &str = "CfxManifestMissingFile";

fn fix(title: impl Into<String>, (start, end, text): Edit) -> Fix {
    Fix::new(title).replace(start, end, text)
}

/// The name of the first directive named `pref`, else of the first directive, else the whole file.
fn anchor(view: &View, len: usize, pref: Option<&str>) -> (usize, usize) {
    pref.and_then(|p| view.first_named(p))
        .or(view.decls.first())
        .map_or((0, len), |d| (d.entry.name_start, d.entry.name_end))
}

fn at_name(code: &'static str, severity: Severity, d: &Decl, message: impl Into<String>) -> Problem {
    Problem::new(code, severity, d.entry.name_start, d.entry.name_end, message)
}

fn at_value(code: &'static str, severity: Severity, v: &Value, message: impl Into<String>) -> Problem {
    Problem::new(code, severity, v.start, v.end, message)
}

pub fn problems(cx: &Ctx) -> Vec<Problem> {
    let src = &cx.doc.src;
    let view = View::new(src);
    let fx = file_name(&cx.doc.path).eq_ignore_ascii_case("fxmanifest.lua");
    let mut out = Vec::new();
    if fx {
        fx_version(src, &view, &mut out);
    }
    let rdr3 = games(src, &view, fx, &mut out);
    if fx && let Some(rdr3) = rdr3 {
        rdr3_warning(src, &view, rdr3, &mut out);
    }
    deprecated(cx, &view, fx, &mut out);
    unknown(&view, &mut out);
    if cx.ws.scanned {
        missing_files(cx, &view, &mut out);
    }
    out
}

fn fx_version(src: &crate::lua::Source, view: &View, out: &mut Vec<Problem>) {
    let text = &src.text;
    let fxs: Vec<&Decl> = view.named("fx_version").collect();
    if fxs.is_empty() {
        let (s, e) = anchor(view, text.len(), None);
        let add = edits::add_directive(src, view, FX_VERSION_LINE, &[]);
        out.push(
            Problem::new(
                FX_VERSION,
                Severity::Error,
                s,
                e,
                "Missing fx_version: FXServer does not start a resource without a manifest version",
            )
            .fix(fix("Add fx_version 'cerulean'", add)),
        );
    }
    for v in fxs.iter().flat_map(|d| d.values()) {
        if !FX_VERSIONS.contains(&v.text.as_str()) {
            let message = format!("Unknown fx_version '{}': FXServer accepts adamant, bodacious, cerulean", v.text);
            let replace = edits::replace_value(text, v, catalog::CURRENT_FX_VERSION);
            out.push(at_value(FX_VERSION, Severity::Error, v, message).fix(fix("Replace with 'cerulean'", replace)));
        }
    }
    for d in fxs.iter().skip(1) {
        let remove = edits::remove_statement(text, d);
        out.push(
            at_name(FX_VERSION, Severity::Warning, d, "Duplicate fx_version: a resource has a single manifest version")
                .fix(fix("Remove the duplicate fx_version", remove)),
        );
    }
}

/// Checks the games; returns the first `'rdr3'` literal.
fn games<'a>(src: &crate::lua::Source, view: &'a View, fx: bool, out: &mut Vec<Problem>) -> Option<&'a Value> {
    let text = &src.text;
    let decls: Vec<&Decl> = view.decls.iter().filter(|d| d.metadata_key() == "game").collect();
    if decls.is_empty() && fx {
        let (s, e) = anchor(view, text.len(), Some("fx_version"));
        let after = ["fx_version"];
        let rdr3 = rdr3_warning_line();
        out.push(
            Problem::new(
                GAME,
                Severity::Error,
                s,
                e,
                "Missing game: FXServer only starts resources that declare the game they run on",
            )
            .fix(fix("Add game 'gta5' (FiveM)", edits::add_directive(src, view, "game 'gta5'", &after)))
            .fix(fix(
                "Add game 'rdr3' (RedM)",
                edits::add_directive(src, view, &format!("game 'rdr3'\n{rdr3}"), &after),
            ))
            .fix(fix(
                "Add games { 'gta5', 'rdr3' } (FiveM and RedM)",
                edits::add_directive(src, view, &format!("games {{ 'gta5', 'rdr3' }}\n{rdr3}"), &after),
            )),
        );
    }
    let literals: Vec<&Value> = decls.iter().flat_map(|d| d.values()).collect();
    let specific = literals.iter().any(|v| matches!(v.text.as_str(), "gta5" | "rdr3" | "gta4"));
    let mut seen: Vec<&str> = Vec::new();
    for v in &literals {
        let game = v.text.as_str();
        if !GAMES.contains(&game) {
            let message = format!("Unknown game '{game}': use gta5 (FiveM), rdr3 (RedM) or common");
            out.push(
                at_value(GAME, Severity::Warning, v, message)
                    .fix(fix("Replace with 'gta5'", edits::replace_value(text, v, "gta5")))
                    .fix(fix("Replace with 'rdr3'", edits::replace_value(text, v, "rdr3"))),
            );
        } else if game == "common" && specific {
            let message =
                "'common' cannot be combined with a specific game: FXServer refuses the resource as ill-formed";
            out.push(at_value(GAME, Severity::Error, v, message));
        } else if seen.contains(&game) {
            out.push(at_value(GAME, Severity::Warning, v, format!("Duplicate game '{game}'")));
        } else {
            seen.push(game);
        }
    }
    literals.into_iter().find(|v| v.text == "rdr3")
}

fn rdr3_warning(src: &crate::lua::Source, view: &View, rdr3: &Value, out: &mut Vec<Problem>) {
    let Some(w) = view.first_named("rdr3_warning") else {
        let message = "RedM resources must declare rdr3_warning with the pre-release acknowledgement, or FXServer refuses to start them";
        let add = edits::add_directive(src, view, &rdr3_warning_line(), &["game", "games"]);
        out.push(at_value(RDR3, Severity::Error, rdr3, message).fix(fix("Add rdr3_warning", add)));
        return;
    };
    match w.values().next() {
        None => out.push(at_name(
            RDR3,
            Severity::Error,
            w,
            "rdr3_warning needs the pre-release acknowledgement as its value",
        )),
        Some(v) if v.text != RDR3_WARNING => {
            let replace = edits::replace_value(&src.text, v, RDR3_WARNING);
            out.push(
                at_value(RDR3, Severity::Error, v, format!("rdr3_warning must be exactly '{RDR3_WARNING}'"))
                    .fix(fix("Replace with the required acknowledgement", replace)),
            );
        }
        Some(_) => {}
    }
}

fn deprecated(cx: &Ctx, view: &View, fx: bool, out: &mut Vec<Problem>) {
    let text = cx.doc.text();
    if !fx {
        // Renaming the file (the original quick fix) is not possible: the problem stays without it.
        let beside = cx.ws.exists(&format!("{}/fxmanifest.lua", parent(&cx.doc.path)));
        let (pref, message) = if beside {
            (None, "This __resource.lua is ignored: FXServer loads the fxmanifest.lua of the same folder")
        } else {
            (
                Some("resource_manifest_version"),
                "Legacy __resource.lua manifest: rename it to fxmanifest.lua and declare fx_version and game",
            )
        };
        let (s, e) = anchor(view, text.len(), pref);
        out.push(Problem::new(DEPRECATED, Severity::Warning, s, e, message));
    }
    for d in &view.decls {
        match d.name() {
            "lua54" => out.push(
                at_name(
                    DEPRECATED,
                    Severity::Warning,
                    d,
                    "lua54 has no effect: every Lua script runs on Lua 5.4 since June 2025",
                )
                .fix(fix("Remove lua54", edits::remove_statement(text, d))),
            ),
            "resource_manifest_version" if fx => {
                let message = "resource_manifest_version belongs to legacy __resource.lua manifests: fxmanifest.lua uses fx_version";
                let f = if view.has("fx_version") {
                    fix("Remove resource_manifest_version", edits::remove_statement(text, d))
                } else {
                    fix("Replace with fx_version 'cerulean'", edits::replace_statement(d, FX_VERSION_LINE))
                };
                out.push(at_name(DEPRECATED, Severity::Warning, d, message).fix(f));
            }
            name @ ("before_level_meta" | "after_level_meta") => {
                out.push(at_name(
                    DEPRECATED,
                    Severity::Warning,
                    d,
                    format!("{name} is deprecated: use data files (data_file) instead"),
                ));
            }
            _ => {}
        }
    }
}

/// The closest directive form within 1 edit (names of up to 5 letters) or 2.
fn suggestion(name: &str) -> Option<&'static str> {
    let max = if name.chars().count() <= 5 { 1 } else { 2 };
    let lower = name.to_lowercase();
    let mut best: Option<(usize, &'static str)> = None;
    for (form, _) in catalog::forms() {
        let d = edit_distance(&lower, form, max);
        if d <= max && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, form.as_str()));
        }
    }
    best.map(|(_, f)| f)
}

fn unknown(view: &View, out: &mut Vec<Problem>) {
    for d in view.decls.iter().filter(|d| d.statement) {
        let name = d.name();
        if catalog::is_table_form(name) && !d.table_form && d.values().next().is_some() {
            let singular = catalog::singular_of(name);
            let message = format!(
                "'{name}' with a single string is recorded as '{name}' and ignored: use '{singular}' or a table"
            );
            out.push(
                at_name(UNKNOWN, Severity::Warning, d, message)
                    .fix(fix(format!("Rename to '{singular}'"), edits::rename(d, singular))),
            );
        } else if !catalog::is_known(name)
            && let Some(s) = suggestion(name)
        {
            let message = format!("Unknown directive '{name}': did you mean '{s}'?");
            out.push(
                at_name(UNKNOWN, Severity::Warning, d, message)
                    .fix(fix(format!("Rename to '{s}'"), edits::rename(d, s))),
            );
        }
    }
}

fn missing_files(cx: &Ctx, view: &View, out: &mut Vec<Problem>) {
    let tree = Tree::new(cx.ws, parent(&cx.doc.path));
    for d in &view.decls {
        for (arg, v) in d.args() {
            let Role::Path(kind) = catalog::role(d.name(), arg) else { continue };
            let value = v.text.as_str();
            if value.trim().is_empty() || (kind.urls() && is_url(value)) {
                continue;
            }
            let message = match split_resource_path(value) {
                Some((res, path)) => {
                    if !kind.other_resources() || path.trim().is_empty() {
                        continue;
                    }
                    let mut dirs = cx.ws.resources().filter(|r| r.name == res).map(|r| r.dir.as_str()).peekable();
                    if dirs.peek().is_none() || dirs.any(|dir| !Tree::new(cx.ws, dir).matches(path, kind).is_empty()) {
                        continue;
                    }
                    if !checkable(path, kind) {
                        continue;
                    }
                    if is_glob(path) {
                        format!("'{path}' matches no file of resource '{res}'")
                    } else {
                        format!("File '{path}' not found in resource '{res}'")
                    }
                }
                None => {
                    if !tree.matches(value, kind).is_empty() || !checkable(value, kind) {
                        continue;
                    }
                    if is_glob(value) {
                        format!("'{value}' matches no file of the resource")
                    } else {
                        format!("File '{value}' not found in the resource")
                    }
                }
            };
            out.push(at_value(MISSING_FILE, Severity::Warning, v, message));
        }
    }
}
