use super::catalog::{self, CONSTRAINTS, PathKind, Role};
use super::files::{Tree, normalize};
use super::snippets::SNIPPETS;
use super::view::{self, Decl, View};
use crate::docs::data::Directive;
use crate::feature::Ctx;
use crate::lua::lexer::Tok;
use crate::project::{Resource, Value};
use crate::text::parent;
use openidea_plugin::providers::{CompletionItem, CompletionKind};

const MAX_GLOB_EXTENSIONS: usize = 8;

pub fn completion(cx: &Ctx, at: usize) -> Vec<CompletionItem> {
    let src = &cx.doc.src;
    if src.in_comment(at) {
        return Vec::new();
    }
    if let Some(i) = src.tok_at(at)
        && src.toks[i].is_string()
        && src.toks[i].start < at
    {
        return values(cx, i, at);
    }
    directives(cx, at)
}

fn sort_text(priority: i32, label: &str) -> String {
    format!("{:03}{label}", 100 - priority)
}

fn item(
    label: &str,
    kind: CompletionKind,
    detail: Option<&str>,
    documentation: Option<String>,
    priority: i32,
) -> CompletionItem {
    let mut it = CompletionItem::new(label, kind);
    it.detail = detail.map(str::to_owned);
    it.documentation = documentation;
    it.sort_text = Some(sort_text(priority, label));
    it
}

// ------------------------------------------------------------------------------------------- directive names

fn directives(cx: &Ctx, at: usize) -> Vec<CompletionItem> {
    let src = &cx.doc.src;
    let text = &src.text;
    let ident = src.tok_at(at).filter(|&i| src.toks[i].kind == Tok::Name && src.toks[i].start < at);
    let (start, end, head) = match ident {
        Some(i) => (src.toks[i].start, src.toks[i].end, i),
        None => {
            // Nothing typed yet: only on an empty spot of the line.
            if at > 0 && !text[..at].ends_with(char::is_whitespace) {
                return Vec::new();
            }
            (at, at, src.toks.partition_point(|t| t.start < at))
        }
    };
    if !view::starts_statement(src, head) || !view::at_depth_zero(src, start) {
        return Vec::new();
    }
    let next = text[end..].trim_start_matches([' ', '\t']).chars().next();
    let has_value = matches!(next, Some('\'' | '"' | '{' | '(' | '['));
    let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let indent: String = text[line_start..start].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    let replace = cx.doc.range(start, end);
    let mut out = Vec::new();
    for (form, d) in catalog::forms() {
        let priority = if d.deprecated {
            -10
        } else if matches!(d.name.as_str(), "fx_version" | "game") {
            5
        } else {
            0
        };
        let mut it =
            item(form, CompletionKind::Property, Some(&catalog::value_kind(d)), Some(documentation(d)), priority);
        if !has_value {
            it.insert_text = format!("{form}{}", shape(form, d, &indent));
        }
        it.replace = Some(replace);
        out.push(it);
    }
    for s in SNIPPETS {
        let mut it = item(s.abbrev, CompletionKind::Snippet, Some(s.description), None, 0);
        it.insert_text = s.expand(&indent);
        it.replace = Some(replace);
        out.push(it);
    }
    out
}

fn shape(form: &str, d: &Directive, indent: &str) -> String {
    let single = match d.values.as_slice() {
        [v] if matches!(d.value_type.as_str(), "enum" | "flag") => Some(v),
        _ => None,
    };
    if form == "games" {
        " { '' }".into()
    } else if catalog::is_table_form(form) || d.value_type == "string[]" {
        format!(" {{\n{indent}    '',\n{indent}}}")
    } else if d.name == "data_file" {
        " '' ''".into()
    } else if d.name == "fx_version" {
        format!(" '{}'", catalog::CURRENT_FX_VERSION)
    } else if let Some(v) = single {
        format!(" '{v}'")
    } else {
        " ''".into()
    }
}

fn documentation(d: &Directive) -> String {
    let mut md = d.description.clone();
    if !d.example.is_empty() {
        md.push_str(&format!("\n\n```lua\n{}\n```", d.example));
    }
    if !d.doc_url.is_empty() {
        md.push_str(&format!("\n\n[Documentation]({})", d.doc_url));
    }
    md
}

// ------------------------------------------------------------------------------------------- values

fn values(cx: &Ctx, tok: usize, at: usize) -> Vec<CompletionItem> {
    let src = &cx.doc.src;
    let (start, end) = src.toks[tok].content_range(&src.text);
    if at < start || at > end {
        return Vec::new();
    }
    let prefix = &src.text[start..at];
    if prefix.contains('\n') {
        return Vec::new();
    }
    let view = View::new(src);
    let Some((decl, arg, value)) = view.value_at(tok) else { return Vec::new() };
    let mut items = match catalog::role(decl.name(), arg) {
        Role::None => Vec::new(),
        Role::Enum(values) => enum_items(decl, value, values),
        Role::Path(kind) => path_items(cx, kind, prefix),
        Role::Dependency => dependency_items(cx, decl, value),
    };
    let replace = cx.doc.range(start, at);
    for it in &mut items {
        it.replace = Some(replace);
    }
    items
}

fn siblings<'a>(decl: &'a Decl, value: &Value) -> Vec<&'a str> {
    if !value.in_table {
        return Vec::new();
    }
    decl.entry.values.iter().filter(|v| v.in_table && v.tok != value.tok).map(|v| v.text.as_str()).collect()
}

fn enum_items(decl: &Decl, value: &Value, values: &[String]) -> Vec<CompletionItem> {
    let used = siblings(decl, value);
    let directive = catalog::find(decl.name()).map_or("", |d| d.name.as_str());
    values
        .iter()
        .enumerate()
        .filter(|(_, v)| !used.contains(&v.as_str()))
        .map(|(i, v)| {
            let tail = value_tail(directive, v).map(str::to_owned);
            item(v, CompletionKind::Constant, Some(decl.name()), tail, (values.len() - i) as i32)
        })
        .collect()
}

fn value_tail(directive: &str, value: &str) -> Option<&'static str> {
    match (directive, value) {
        ("fx_version", "cerulean") => Some("current"),
        ("fx_version", "bodacious") => Some("2020-02"),
        ("fx_version", "adamant") => Some("2019-12, minimum for RedM"),
        ("game", "gta5") => Some("FiveM"),
        ("game", "rdr3") => Some("RedM (requires rdr3_warning)"),
        ("game", "common") => Some("any game, Cfx APIs only"),
        _ => None,
    }
}

fn other_resources<'a>(cx: &'a Ctx) -> Vec<&'a Resource> {
    let own = parent(&cx.doc.path);
    let mut out: Vec<&Resource> = cx.ws.resources().filter(|r| r.dir != own).collect();
    out.sort_by_key(|r| r.name.to_lowercase());
    out.dedup_by(|a, b| a.name == b.name);
    out
}

fn path_items(cx: &Ctx, kind: PathKind, prefix: &str) -> Vec<CompletionItem> {
    let resource_items = |priority: i32| -> Vec<CompletionItem> {
        other_resources(cx)
            .into_iter()
            .map(|r| item(&format!("@{}/", r.name), CompletionKind::Module, Some("other resource"), None, priority))
            .collect()
    };
    if let Some(rest) = prefix.strip_prefix('@') {
        if !kind.other_resources() {
            return Vec::new();
        }
        let Some((name, rel)) = rest.split_once('/') else { return resource_items(0) };
        return cx
            .ws
            .resources()
            .filter(|r| r.name == name)
            .flat_map(|r| children(&Tree::new(cx.ws, &r.dir), rel, &format!("@{name}/"), &r.manifest_path(), kind))
            .collect();
    }
    let mut items = children(&Tree::new(cx.ws, parent(&cx.doc.path)), prefix, "", &cx.doc.path, kind);
    if kind == PathKind::Script && !prefix.contains('/') {
        items.extend(resource_items(-5));
    }
    items
}

fn plural_files(n: usize) -> String {
    if n == 1 { "1 file".into() } else { format!("{n} files") }
}

fn children(tree: &Tree, rel: &str, lookup: &str, manifest: &str, kind: PathKind) -> Vec<CompletionItem> {
    let norm = normalize(rel);
    let dir = norm.rfind('/').map_or("", |i| &norm[..=i]);
    let typed = rel.rfind('/').map_or("", |i| &rel[..=i]);
    let (dirs, names) = tree.children(dir);
    let mut out = Vec::new();
    for d in dirs.iter().filter(|d| !d.starts_with('.')) {
        let priority = if d == "node_modules" { -3 } else { 2 };
        out.push(item(&format!("{lookup}{typed}{d}/"), CompletionKind::Folder, None, None, priority));
    }
    let ext = |n: &str| n.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    let files: Vec<&String> =
        names.iter().filter(|n| !n.starts_with('.') && format!("{}/{dir}{n}", tree.base) != manifest).collect();
    let wanted = kind.extensions();
    for n in &files {
        let Some(e) = ext(n) else { continue };
        if !wanted.is_empty() && !wanted.contains(&e.as_str()) {
            continue;
        }
        let detail = (!dir.is_empty()).then_some(dir);
        out.push(item(&format!("{lookup}{typed}{n}"), CompletionKind::File, detail, None, 1));
    }
    if kind == PathKind::Page {
        return out;
    }
    let deep = |e: &str| {
        let own = manifest.strip_prefix(&format!("{}/", tree.base)).unwrap_or(manifest);
        tree.matches(&format!("{dir}**/*.{e}"), kind).iter().filter(|m| !m.dir && m.rel != own).count()
    };
    let mut exts: Vec<String> = Vec::new();
    if kind == PathKind::Script {
        for e in wanted {
            if files.iter().any(|n| ext(n).as_deref() == Some(*e)) || deep(e) > 0 {
                exts.push((*e).to_owned());
            }
        }
    } else {
        for e in files.iter().filter_map(|n| ext(n)) {
            if !exts.contains(&e) {
                exts.push(e);
            }
        }
    }
    for e in exts.iter().take(MAX_GLOB_EXTENSIONS) {
        let direct = files.iter().filter(|n| ext(n).as_deref() == Some(e.as_str())).count();
        if direct > 0 {
            out.push(item(
                &format!("{lookup}{typed}*.{e}"),
                CompletionKind::Snippet,
                Some("glob"),
                Some(plural_files(direct)),
                0,
            ));
        }
        let recursive = deep(e);
        if recursive > direct {
            out.push(item(
                &format!("{lookup}{typed}**/*.{e}"),
                CompletionKind::Snippet,
                Some("glob"),
                Some(plural_files(recursive)),
                0,
            ));
        }
    }
    out
}

fn dependency_items(cx: &Ctx, decl: &Decl, value: &Value) -> Vec<CompletionItem> {
    let used = siblings(decl, value);
    let mut out: Vec<CompletionItem> = other_resources(cx)
        .into_iter()
        .filter(|r| !used.contains(&r.name.as_str()))
        .map(|r| item(&r.name, CompletionKind::Module, Some("resource"), None, 1))
        .collect();
    for (c, description) in CONSTRAINTS {
        if !used.contains(&c) {
            out.push(item(c, CompletionKind::Constant, Some("constraint"), Some(description.to_owned()), 0));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::Feature;
    use crate::manifest::ManifestFeature;
    use crate::testing::Fixture;

    const RES: &[(&str, &str)] = &[
        ("res/client/main.lua", ""),
        ("res/client/utils.lua", ""),
        ("res/client/sub/deep.lua", ""),
        ("res/server/main.lua", ""),
        ("res/config.lua", ""),
        ("res/README.txt", ""),
        ("res/html/index.html", ""),
        ("res/html/app.js", ""),
        ("res/html/style.css", ""),
        ("res/html/logo.png", ""),
        ("res/data/handling.meta", ""),
        ("other/fxmanifest.lua", "fx_version 'cerulean'"),
        ("other/init.lua", ""),
        ("other/lib/x.lua", ""),
        ("third/fxmanifest.lua", "fx_version 'cerulean'"),
    ];

    fn complete(manifest: &str) -> Vec<CompletionItem> {
        let mut files = RES.to_vec();
        files.push(("res/fxmanifest.lua", manifest));
        let fx = Fixture::new(&files);
        let (doc, at) = fx.caret("res/fxmanifest.lua");
        ManifestFeature.completion(&fx.cx(&doc), at, None)
    }

    fn labels(manifest: &str) -> Vec<String> {
        complete(manifest).into_iter().map(|i| i.label).collect()
    }

    fn pick(manifest: &str, label: &str) -> String {
        let mut files = RES.to_vec();
        files.push(("res/fxmanifest.lua", manifest));
        let fx = Fixture::new(&files);
        let (doc, at) = fx.caret("res/fxmanifest.lua");
        let items = ManifestFeature.completion(&fx.cx(&doc), at, None);
        let it = items.iter().find(|i| i.label == label).unwrap_or_else(|| panic!("no {label}"));
        let r = it.replace.unwrap();
        let (s, e) = (doc.offset(r.start), doc.offset(r.end));
        format!("{}{}{}", &doc.text()[..s], it.insert_text, &doc.text()[e..])
    }

    #[test]
    fn directive_shapes() {
        assert_eq!(pick("fx_vers<caret>", "fx_version"), "fx_version 'cerulean'");
        assert_eq!(pick("client_scripts<caret>", "client_scripts"), "client_scripts {\n    '',\n}");
        assert_eq!(
            pick("if true then\n    server_scripts<caret>\nend", "server_scripts"),
            "if true then\n    server_scripts {\n        '',\n    }\nend"
        );
        assert_eq!(pick("games<caret>", "games"), "games { '' }");
        assert_eq!(pick("descript<caret>", "description"), "description ''");
        assert_eq!(pick("data_fi<caret>", "data_file"), "data_file '' ''");
        assert_eq!(pick("rdr3_w<caret>", "rdr3_warning"), format!("rdr3_warning '{}'", catalog::RDR3_WARNING));
        assert_eq!(pick("client_scr<caret> 'client/main.lua'", "client_script"), "client_script 'client/main.lua'");
        assert_eq!(pick("escrow_<caret>", "escrow_ignore"), "escrow_ignore {\n    '',\n}");
        assert_eq!(pick("ui_page_pre<caret>", "ui_page_preload"), "ui_page_preload 'yes'");
        assert_eq!(pick("fx_version 'cerulean'\n<caret>", "game"), "fx_version 'cerulean'\ngame ''");
    }

    #[test]
    fn directive_items_and_context() {
        let items = complete("<caret>");
        let fx = items.iter().find(|i| i.label == "fx_version").unwrap();
        assert_eq!(fx.detail.as_deref(), Some("enum"));
        assert!(fx.documentation.as_deref().unwrap().contains("```lua\nfx_version 'cerulean'\n```"));
        let lua54 = items.iter().find(|i| i.label == "lua54").unwrap();
        assert!(fx.sort_text < items.iter().find(|i| i.label == "author").unwrap().sort_text);
        assert!(lua54.sort_text > items.iter().find(|i| i.label == "author").unwrap().sort_text);
        assert!(items.iter().any(|i| i.label == "fxmr" && i.kind == Some(CompletionKind::Snippet)));
        assert!(labels("client_scripts { cl<caret> }").is_empty(), "not inside tables");
        assert!(labels("local x = cl<caret>").is_empty(), "not in expressions");
        assert!(labels("-- cl<caret>").is_empty(), "not in comments");
        assert!(labels("fx_version 'cerulean'<caret>").is_empty());
        assert!(labels("x.cl<caret>").is_empty());
    }

    #[test]
    fn enum_values() {
        assert_eq!(labels("fx_version '<caret>'"), ["cerulean", "bodacious", "adamant"]);
        assert_eq!(labels("node_version '<caret>'"), ["16", "22"]);
        let games = complete("games { 'gta5', '<caret>' }");
        assert_eq!(games.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["rdr3", "common"]);
        assert_eq!(games[0].detail.as_deref(), Some("games"));
        assert_eq!(games[0].documentation.as_deref(), Some("RedM (requires rdr3_warning)"));
        let types = labels("data_file 'veh<caret>'");
        assert!(types.contains(&"VEHICLE_METADATA_FILE".to_owned()) && types.contains(&"HANDLING_FILE".to_owned()));
        assert!(labels("description '<caret>'").is_empty());
        assert!(labels("data_file 'HANDLING_FILE' 'x' '<caret>'").is_empty());
    }

    #[test]
    fn paths() {
        let l = labels("client_script '<caret>'");
        for want in ["client/", "server/", "html/", "config.lua", "*.lua", "**/*.lua", "@other/", "@third/"] {
            assert!(l.contains(&want.to_owned()), "{want} in {l:?}");
        }
        for not in ["README.txt", "fxmanifest.lua", "@res/", "*.txt"] {
            assert!(!l.contains(&not.to_owned()), "{not} in {l:?}");
        }
        let l = labels("client_scripts { 'client/<caret>' }");
        assert_eq!(l, ["client/sub/", "client/main.lua", "client/utils.lua", "client/*.lua", "client/**/*.lua"]);
        let l = labels("files { 'html/<caret>' }");
        for want in [
            "html/index.html",
            "html/app.js",
            "html/style.css",
            "html/logo.png",
            "html/*.js",
            "html/*.css",
            "html/*.png",
            "html/*.html",
        ] {
            assert!(l.contains(&want.to_owned()), "{want} in {l:?}");
        }
        let l = labels("ui_page '<caret>'");
        assert!(
            l.contains(&"html/".to_owned())
                && !l.iter().any(|x| x == "config.lua" || x == "README.txt" || x == "*.lua")
        );
        assert_eq!(labels("ui_page 'html/<caret>'"), ["html/index.html"]);
        assert_eq!(labels("data_file 'HANDLING_FILE' 'data/<caret>'"), ["data/handling.meta", "data/*.meta"]);
        assert_eq!(labels("shared_script '@<caret>'"), ["@other/", "@third/"]);
        assert_eq!(
            labels("shared_script '@other/<caret>'"),
            ["@other/lib/", "@other/init.lua", "@other/*.lua", "@other/**/*.lua"]
        );
        assert!(labels("files { '@<caret>' }").is_empty());
        assert_eq!(labels("client_script './client/<caret>'")[0], "./client/sub/");
        let globs = complete("client_script 'client/<caret>'");
        let deep = globs.iter().find(|i| i.label == "client/**/*.lua").unwrap();
        assert_eq!((deep.detail.as_deref(), deep.documentation.as_deref()), (Some("glob"), Some("3 files")));
    }

    #[test]
    fn dependencies() {
        let l = labels("dependencies { '<caret>' }");
        assert_eq!(l[..2], ["other".to_owned(), "third".to_owned()]);
        for want in ["/onesync", "/server:", "/gameBuild:", "/policy:subdir_file_mapping", "/native:"] {
            assert!(l.contains(&want.to_owned()), "{want}");
        }
        assert!(!l.contains(&"res".to_owned()));
        let l = labels("dependencies { 'other', '/onesync', '<caret>' }");
        assert!(
            !l.contains(&"other".to_owned()) && !l.contains(&"/onesync".to_owned()) && l.contains(&"third".to_owned())
        );
    }
}
