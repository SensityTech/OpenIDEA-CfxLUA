use super::build::{Configs, dir_exists, join};
use crate::feature::{Doc, Fix, Intention, Problem};
use crate::lua::Source;
use crate::project::glob::Glob;
use crate::project::{CASE_SENSITIVE, Entry, Value};
use crate::syntax::tree::Tree;
use crate::text::{file_name, parent};
use crate::workspace::Workspace;
use openidea_plugin::Severity;

const CODE: &str = "CfxNuiFilesMissing";
const MAX_FILES: usize = 2000;
const MAX_EXAMPLES: usize = 3;
const NOT_SERVED_EXTENSIONS: &[&str] = &[
    "map", "md", "markdown", "txt", "log", "lock", "scss", "sass", "less", "styl", "ts", "tsx", "mts", "cts", "jsx",
    "vue", "svelte", "psd", "xcf", "ai", "sketch", "fig",
];
const NOT_SERVED_NAMES: &[&str] = &[
    "asset-manifest.json",
    "package.json",
    "package-lock.json",
    "pnpm-lock.yaml",
    "tsconfig.json",
    "thumbs.db",
    "desktop.ini",
    "license",
    "readme",
];
const MARKER: &str = "-- ui_page build: ";

fn file_entries(entries: &[Entry]) -> Vec<&Entry> {
    entries.iter().filter(|e| e.name == "file" || (e.name == "files" && e.table.is_some())).collect()
}

fn normalize(value: &str) -> String {
    let p = value.trim().replace('\\', "/");
    let p = p.trim_start_matches("./");
    p.trim_start_matches('/').to_owned()
}

fn is_url(value: &str) -> bool {
    value.contains("://")
}

fn is_source_project(ws: &Workspace, dir: &str) -> bool {
    ws.exists(&format!("{dir}/package.json"))
        || dir_exists(ws, &format!("{dir}/node_modules"))
        || ws.files_under(dir).any(|f| {
            let rel = &f.path[dir.len() + 1..];
            !rel.contains('/') && (rel.starts_with("vite.config.") || rel.starts_with("webpack.config."))
        })
}

fn is_not_served(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let ext = name.rsplit_once('.').map_or("", |(_, e)| e);
    NOT_SERVED_NAMES.contains(&name.as_str()) || NOT_SERVED_EXTENSIONS.contains(&ext) || name.ends_with(".d.ts")
}

fn build_root(ws: &Workspace, configs: &Configs, res: &str, page: &str) -> Option<String> {
    let inside = |outer: &str, inner: &str| inner.strip_prefix(outer).is_some_and(|r| r.starts_with('/'));
    for (dir, c) in configs.under(res) {
        if let Some(out) = join(dir, &c.out_dir)
            && dir_exists(ws, &out)
            && inside(res, &out)
            && inside(&out, page)
        {
            return Some(out);
        }
    }
    let dir = parent(page);
    (dir != res && inside(res, dir) && !is_source_project(ws, dir)).then(|| dir.to_owned())
}

pub fn files_missing(doc: &Doc, ws: &Workspace, configs: &Configs) -> Vec<Problem> {
    let src = &doc.src;
    let res = parent(&doc.path);
    let entries = crate::project::manifest::entries(src);
    let globs: Vec<Glob> = file_entries(&entries)
        .iter()
        .flat_map(|e| &e.values)
        .filter_map(|v| Glob::new(&v.text, CASE_SENSITIVE))
        .collect();
    let covered = |rel: &str| globs.iter().any(|g| g.matches(rel));
    let mut out = Vec::new();
    for entry in entries.iter().filter(|e| e.name == "ui_page") {
        let Some(literal) = entry.first() else { continue };
        let value = &literal.text;
        if value.trim().is_empty() || is_url(value) || value.starts_with('@') {
            continue;
        }
        let page = normalize(value);
        let page_path = format!("{res}/{page}");
        if !covered(&page) {
            // A page that does not exist is reported by the manifest checks.
            if !ws.exists(&page_path) {
                continue;
            }
            let glob = match page.rsplit_once('/') {
                None => page.clone(),
                Some((dir, _)) if is_source_project(ws, &format!("{res}/{dir}")) => page.clone(),
                Some((dir, _)) => format!("{dir}/**"),
            };
            let message = format!(
                "'{value}' is not covered by files: FXServer does not send the page to the game and the NUI stays white"
            );
            out.push(
                Problem::new(CODE, Severity::Warning, literal.start, literal.end, message)
                    .fix(add_files(src, &entries, literal, &glob)),
            );
            continue;
        }
        if !ws.exists(&page_path) {
            continue;
        }
        let Some(root) = build_root(ws, configs, res, &page_path) else { continue };
        let mut missing = Vec::new();
        let mut count = 0;
        for f in ws.files_under(&root) {
            let rel_root = &f.path[root.len() + 1..];
            let segments: Vec<&str> = rel_root.split('/').collect();
            let hidden = segments.iter().any(|s| s.starts_with('.'));
            let dependency = segments[..segments.len() - 1].contains(&"node_modules");
            if hidden || dependency || is_not_served(file_name(&f.path)) {
                continue;
            }
            count += 1;
            if count > MAX_FILES {
                break;
            }
            let rel = &f.path[res.len() + 1..];
            if !covered(rel) {
                missing.push(rel.to_owned());
            }
        }
        if missing.is_empty() {
            continue;
        }
        let root_rel = &root[res.len() + 1..];
        let prefix = format!("{root_rel}/");
        let examples: Vec<&str> =
            missing.iter().take(MAX_EXAMPLES).map(|m| m.strip_prefix(&prefix).unwrap_or(m)).collect();
        let files = if missing.len() == 1 { "1 file".to_owned() } else { format!("{} files", missing.len()) };
        let message = format!(
            "{files} of '{root_rel}' not covered by files ({}): the page loads without them",
            examples.join(", ")
        );
        let glob = format!("{root_rel}/**");
        out.push(
            Problem::new(CODE, Severity::Warning, literal.start, literal.end, message)
                .fix(add_files(src, &entries, literal, &glob)),
        );
    }
    out
}

fn quote_of(src: &Source, v: Option<&Value>) -> char {
    if v.is_some_and(|v| src.text[v.start..].starts_with('"')) { '"' } else { '\'' }
}

fn line_end(text: &str, at: usize) -> usize {
    text[at..].find(['\n', '\r']).map_or(text.len(), |i| at + i)
}

fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |i| i + 1)
}

fn add_files(src: &Source, entries: &[Entry], literal: &Value, glob: &str) -> Fix {
    let fix = Fix::new(format!("Add '{glob}' to files"));
    let text = &src.text;
    let files = file_entries(entries);
    if let Some(e) = files.iter().find(|e| e.table.is_some())
        && let Some((open, Some(close))) = e.table
    {
        let q = quote_of(src, e.values.first());
        let quoted = format!("{q}{glob}{q}");
        let fields = super::lua::table_fields(src, &Tree::new(src), open);
        let Some(&(_, last_end)) = fields.last() else {
            return fix.insert(src.toks[open].end, format!(" {quoted} "));
        };
        let multiline = text[src.toks[open].start..src.toks[close].end].contains('\n');
        let last_start = src.toks[fields[fields.len() - 1].0].start;
        let ls = line_start(text, last_start);
        let indent: String = text[ls..].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        if src.is_op(last_end, ",") || src.is_op(last_end, ";") {
            let at = src.toks[last_end].end;
            return fix.insert(at, if multiline { format!("\n{indent}{quoted},") } else { format!(" {quoted},") });
        }
        let at = src.toks[last_end - 1].end;
        return fix.insert(at, if multiline { format!(",\n{indent}{quoted}") } else { format!(", {quoted}") });
    }
    let q = quote_of(src, Some(literal));
    let quoted = format!("{q}{glob}{q}");
    let anchor_end = match files.last() {
        Some(e) => e.end,
        None => entries.iter().find(|e| e.values.first() == Some(literal)).map_or(literal.end, |e| e.end),
    };
    let line = if files.is_empty() { format!("files {{\n    {quoted},\n}}") } else { format!("file {quoted}") };
    fix.insert(line_end(text, anchor_end), format!("\n{line}"))
}

enum Line<'a> {
    Build(&'a Value),
    Dev { literal: &'a Value, original: String, comment: usize, end: usize },
}

fn line_at<'a>(text: &str, entries: &'a [Entry], at: usize) -> Option<Line<'a>> {
    for e in entries.iter().filter(|e| e.name == "ui_page") {
        let Some(literal) = e.first() else { continue };
        let ls = line_start(text, e.start);
        let le = line_end(text, e.end);
        if at < ls || at > le {
            continue;
        }
        if !is_url(&literal.text) {
            return (!literal.text.trim().is_empty()).then_some(Line::Build(literal));
        }
        let rest = &text[literal.end..le];
        let marker = rest.rfind(&format!(" {MARKER}"))?;
        let original = &rest[marker + 1 + MARKER.len()..];
        if original.trim().is_empty() {
            return None;
        }
        return Some(Line::Dev { literal, original: original.to_owned(), comment: literal.end + marker, end: le });
    }
    None
}

pub fn dev_mode(doc: &Doc, configs: &Configs, default_port: u16, at: usize) -> Option<Intention> {
    let src = &doc.src;
    let text = &src.text;
    let entries = crate::project::manifest::entries(src);
    let fix = match line_at(text, &entries, at)? {
        Line::Build(literal) => {
            let url = format!("http://localhost:{}", configs.dev_port(parent(&doc.path), default_port));
            let end = line_end(text, literal.end);
            let q = quote_of(src, Some(literal));
            let original = &text[literal.start..literal.end];
            let dev = format!("{q}{url}{q}{} {MARKER}{original}", &text[literal.end..end]);
            Fix::new(format!("Switch ui_page to the dev server ({url})")).replace(literal.start, end, dev)
        }
        Line::Dev { literal, original, comment, end } => {
            let build = format!("{original}{}", &text[literal.end..comment]);
            Fix::new(format!("Switch ui_page back to the build ({original})")).replace(literal.start, end, build)
        }
    };
    Some(Intention { fix, kind: "refactor" })
}
