//! Bundler configurations of UI projects (`vite.config.ts`,
//! `webpack.config.js`), read from their text: where they build (a UI kept
//! outside its resource belongs to the resource holding its output folder)
//! and the port of their dev server.

use crate::text::{file_name, parent};
use crate::workspace::Workspace;
use std::collections::BTreeMap;

const VITE_NAMES: &[&str] =
    &["vite.config.ts", "vite.config.js", "vite.config.mts", "vite.config.mjs", "vite.config.cts", "vite.config.cjs"];
const WEBPACK_NAMES: &[&str] = &["webpack.config.js", "webpack.config.ts", "webpack.config.cjs", "webpack.config.mjs"];
const SKIPPED_DIRS: &[&str] = &["node_modules", ".git", ".idea"];
/// Directory levels below a resource searched for configurations.
const MAX_DEPTH: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// The build output directory, relative to the configuration's folder.
    pub out_dir: String,
    /// The dev server port it declares.
    pub port: Option<u16>,
}

impl Config {
    pub fn parse(text: &str, vite: bool) -> Self {
        let out = if vite { assigned_path(text, "outDir", false) } else { assigned_path(text, "path", true) };
        let out = out.unwrap_or_else(|| "dist".to_owned());
        let out = out.trim();
        let out = out.strip_prefix("./").unwrap_or(out).trim_end_matches('/');
        Self { out_dir: out.to_owned(), port: server_port(text, vite) }
    }
}

fn word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn skip_ws(s: &[u8], mut i: usize) -> usize {
    while s.get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    i
}

/// `literal` at `i`, returning the index after it.
fn expect(s: &[u8], i: usize, literal: &str) -> Option<usize> {
    s.get(i..i + literal.len()).filter(|w| *w == literal.as_bytes()).map(|_| i + literal.len())
}

/// `(path.)resolve(__dirname, ` / `join(__dirname, ` at `i`.
fn from_dirname(s: &[u8], i: usize) -> Option<usize> {
    let mut j = i;
    if let Some(k) = expect(s, j, "path") {
        let k = skip_ws(s, k);
        if let Some(k) = expect(s, k, ".") {
            j = skip_ws(s, k);
        }
    }
    let j = expect(s, j, "resolve").or_else(|| expect(s, j, "join"))?;
    let j = skip_ws(s, expect(s, j, "(")?);
    let j = skip_ws(s, expect(s, j, "__dirname")?);
    Some(skip_ws(s, expect(s, j, ",")?))
}

fn quoted(s: &[u8], i: usize) -> Option<String> {
    let quote = |b: &u8| matches!(b, b'\'' | b'"' | b'`');
    if !s.get(i).is_some_and(quote) {
        return None;
    }
    let len = s[i + 1..].iter().position(quote)?;
    (len > 0).then(|| String::from_utf8_lossy(&s[i + 1..i + 1 + len]).into_owned())
}

/// The first `key: 'dir'` (or `key: path.resolve(__dirname, 'dir')`; `dirname` makes that form required).
fn assigned_path(text: &str, key: &str, dirname: bool) -> Option<String> {
    let s = text.as_bytes();
    let mut from = 0;
    while let Some(at) = text.get(from..).and_then(|t| t.find(key)).map(|i| from + i) {
        from = at + key.len();
        if at > 0 && word(s[at - 1]) {
            continue;
        }
        let Some(colon) = expect(s, skip_ws(s, from), ":") else { continue };
        let value = skip_ws(s, colon);
        let found = match from_dirname(s, value) {
            Some(v) => quoted(s, v),
            None if !dirname => quoted(s, value),
            None => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

/// The `port` of the dev server block (`server: { port: 3000 }`), at the top level of the block only.
pub fn server_port(text: &str, vite: bool) -> Option<u16> {
    let key = if vite { "server" } else { "devServer" };
    let s = text.as_bytes();
    let mut from = 0;
    while let Some(at) = text.get(from..).and_then(|t| t.find(key)).map(|i| from + i) {
        from = at + key.len();
        if at > 0 && word(s[at - 1]) {
            continue;
        }
        let Some(colon) = expect(s, skip_ws(s, from), ":") else { continue };
        let Some(open) = expect(s, skip_ws(s, colon), "{") else { continue };
        let mut depth = 1;
        let mut i = open;
        while i < s.len() && depth > 0 {
            match s[i] {
                b'{' | b'[' | b'(' => depth += 1,
                b'}' | b']' | b')' => depth -= 1,
                b'p' if depth == 1 && (i == 0 || !word(s[i - 1])) => {
                    if let Some(port) = port_at(s, i) {
                        return port;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }
    None
}

/// `port: 1234` at `i`: `Some(None)` for a literal out of 1..=65535.
fn port_at(s: &[u8], i: usize) -> Option<Option<u16>> {
    let j = skip_ws(s, expect(s, i, "port")?);
    let j = skip_ws(s, expect(s, j, ":")?);
    let len = s[j..].iter().take_while(|b| b.is_ascii_digit()).count();
    if !(2..=5).contains(&len) || s.get(j + len).is_some_and(|b| word(*b)) {
        return None;
    }
    let v: u32 = std::str::from_utf8(&s[j..j + len]).ok()?.parse().ok()?;
    Some(u16::try_from(v).ok().filter(|p| *p >= 1))
}

/// The configurations of the project, by path.
#[derive(Default)]
pub struct Configs(BTreeMap<String, Config>);

impl Configs {
    /// Reads a configuration file (other `*.config.*` names are not configurations).
    pub fn set(&mut self, path: &str, text: &str) {
        let name = file_name(path);
        if VITE_NAMES.contains(&name) {
            self.0.insert(path.to_owned(), Config::parse(text, true));
        } else if WEBPACK_NAMES.contains(&name) {
            self.0.insert(path.to_owned(), Config::parse(text, false));
        } else {
            self.0.remove(path);
        }
    }

    pub fn remove(&mut self, path: &str) {
        self.0.remove(path);
    }

    /// The configuration of directory `dir` itself (Vite first, in name order).
    pub fn in_dir(&self, dir: &str) -> Option<&Config> {
        VITE_NAMES.iter().chain(WEBPACK_NAMES).find_map(|n| self.0.get(&format!("{dir}/{n}")))
    }

    /// The configurations under `dir` (a few levels deep, dependencies left out), in path order,
    /// with their folder.
    pub fn under<'a>(&'a self, dir: &str) -> impl Iterator<Item = (&'a str, &'a Config)> + 'a {
        let prefix = format!("{dir}/");
        let len = prefix.len();
        self.0
            .range(prefix.clone()..)
            .take_while(move |(p, _)| p.starts_with(&prefix))
            .filter(move |(p, _)| {
                let rel = &p[len..];
                let dirs: Vec<&str> = rel.split('/').collect();
                dirs.len() <= MAX_DEPTH + 1 && !dirs[..dirs.len() - 1].iter().any(|d| SKIPPED_DIRS.contains(d))
            })
            .map(|(p, c)| (parent(p), c))
    }

    /// The dev server port of the UI of the resource at `dir`: a configuration's, else `default`.
    pub fn dev_port(&self, dir: &str, default: u16) -> u16 {
        self.under(dir).find_map(|(_, c)| c.port).unwrap_or(default)
    }

    /// The resource a web file outside every resource belongs to: the one holding the output folder of
    /// the nearest configuration above it.
    pub fn build_target(&self, ws: &Workspace, path: &str) -> Option<String> {
        let mut dir = parent(path);
        while within(&ws.root, dir) {
            if let Some(c) = self.in_dir(dir) {
                let out = existing_ancestor(ws, dir, &c.out_dir)?;
                return resource_holding(ws, &out);
            }
            if dir == ws.root {
                break;
            }
            dir = parent(dir);
        }
        None
    }
}

/// Whether `dir` is `root` or inside it.
pub fn within(root: &str, dir: &str) -> bool {
    !dir.is_empty() && (dir == root || dir.strip_prefix(root).is_some_and(|r| r.starts_with('/')))
}

/// Whether a directory exists: the workspace knows a file under it.
pub fn dir_exists(ws: &Workspace, dir: &str) -> bool {
    !dir.is_empty() && ws.files_under(dir).next().is_some()
}

/// `base` joined with the relative path `rel` (`..` and `.` resolved), `None` above the file system root.
pub fn join(base: &str, rel: &str) -> Option<String> {
    let mut dir = base.to_owned();
    for seg in rel.split('/').filter(|s| !s.is_empty() && *s != ".") {
        if seg == ".." {
            let p = parent(&dir);
            if p.is_empty() {
                return None;
            }
            dir = p.to_owned();
        } else {
            dir = format!("{dir}/{seg}");
        }
    }
    Some(dir)
}

/// The deepest existing folder of `rel` (`../res/html`) from `base`.
fn existing_ancestor(ws: &Workspace, base: &str, rel: &str) -> Option<String> {
    let mut dir = base.to_owned();
    for seg in rel.split('/').filter(|s| !s.is_empty() && *s != ".") {
        let next = if seg == ".." {
            Some(parent(&dir).to_owned()).filter(|p| !p.is_empty())
        } else {
            Some(format!("{dir}/{seg}"))
        };
        match next.filter(|n| dir_exists(ws, n)) {
            Some(n) => dir = n,
            None => return (seg != "..").then_some(dir),
        }
    }
    Some(dir)
}

/// The resource folder (holding a manifest) containing `dir`, within the project.
fn resource_holding(ws: &Workspace, dir: &str) -> Option<String> {
    let mut current = dir;
    while within(&ws.root, current) {
        if ws.resource(current).is_some() {
            return Some(current.to_owned());
        }
        if current == ws.root {
            break;
        }
        current = parent(current);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_port_of_the_server_block_not_of_a_nested_block() {
        assert_eq!(server_port("export default { server: { hmr: { port: 24678 }, port: 3000 } }", true), Some(3000));
        assert_eq!(server_port("server: {\n  proxy: { '/api': { target: 'x' } },\n  port: 3000,\n}", true), Some(3000));
        assert_eq!(
            server_port("module.exports = { devServer: { client: { port: 1 }, port: 8080 } }", false),
            Some(8080)
        );
    }

    #[test]
    fn no_port_outside_of_the_server_block() {
        assert_eq!(server_port("export default { preview: { port: 4173 }, server: { open: true } }", true), None);
        assert_eq!(server_port("export default { server: { hmr: { port: 24678 } } }", true), None);
        assert_eq!(server_port("export default { server: { port: Number(process.env.PORT) } }", true), None);
        assert_eq!(server_port("server: { port: 99999 }", true), None);
    }

    #[test]
    fn output_folders() {
        assert_eq!(Config::parse("build: { outDir: './build/' }", true).out_dir, "build");
        assert_eq!(Config::parse("outDir: path.resolve(__dirname, '../hud/html')", true).out_dir, "../hud/html");
        assert_eq!(Config::parse("plugins: []", true).out_dir, "dist");
        assert_eq!(Config::parse("output: { path: path.resolve(__dirname, 'html') }", false).out_dir, "html");
        assert_eq!(Config::parse("output: { path: 'html' }", false).out_dir, "dist", "webpack needs __dirname");
        assert_eq!(join("/p/ui", "../uires/./html"), Some("/p/uires/html".into()));
    }

    #[test]
    fn ui_projects_outside_of_their_resource() {
        let ws = Workspace::from_files(
            "/p",
            &[
                ("uires/fxmanifest.lua", "ui_page 'html/index.html'"),
                (
                    "ui/vite.config.ts",
                    "export default defineConfig({ build: { outDir: '../uires/html', emptyOutDir: true } })",
                ),
                ("ui/src/main.ts", "fetchNui('x')"),
                ("hud/fxmanifest.lua", ""),
                ("hud/html/index.html", "<html></html>"),
                (
                    "hud-ui/vite.config.ts",
                    "import path from 'path'\nexport default { build: { outDir: path.resolve(__dirname, '../hud/html') } }",
                ),
                ("hud-ui/src/main.ts", ""),
                ("loose/src/a.ts", ""),
            ],
        );
        let mut configs = Configs::default();
        for (path, text) in ws.texts(crate::project::FileKind::BundlerConfig) {
            configs.set(path, text);
        }
        assert_eq!(configs.build_target(&ws, "/p/ui/src/main.ts").as_deref(), Some("/p/uires"));
        assert_eq!(configs.build_target(&ws, "/p/hud-ui/src/main.ts").as_deref(), Some("/p/hud"));
        assert_eq!(configs.build_target(&ws, "/p/loose/src/a.ts"), None);
        assert_eq!(configs.dev_port("/p/hud", 5173), 5173);
    }
}
