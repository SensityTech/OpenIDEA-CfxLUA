// Asks a plugin package what the editor would receive for a file, through OpenIDEA's own plugin host and without
// the editor: semantic tokens, folds, structure and the occurrences of a word. The calls run under the host's limits
// for editor calls, so a provider that is too slow answers nothing here as it would in the IDE.
// Built like scripts/openidea-start-check.rs, as an example of OpenIDEA's `ide-host` crate:
//
//   cp scripts/openidea-editor-check.rs <openidea>/crates/ide-host/examples/cfxlua_editor_check.rs
//   cargo run --release --locked -p ide-host --example cfxlua_editor_check -- <package.oiplugin> <file.lua> [word]

use ide_core::Pos;
use ide_host::plugins::{Consent, PluginHost};
use ide_plugin::{Limits, ProviderDocument};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(package), Some(file)) = (args.first(), args.get(1)) else {
        eprintln!("usage: cfxlua_editor_check <package.oiplugin> <file.lua> [word]");
        return ExitCode::from(2);
    };
    let dir = std::env::temp_dir().join(format!("cfxlua-editor-check-{}", std::process::id()));
    let result = check(&dir, Path::new(package), Path::new(file), args.get(2).map(String::as_str));
    let _ = std::fs::remove_dir_all(&dir);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            println!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// The text a range of one line covers; columns count UTF-16 units.
fn slice(lines: &[&str], line: u32, from: u32, to: u32) -> String {
    let units: Vec<u16> = lines.get(line as usize).map(|l| l.encode_utf16().collect()).unwrap_or_default();
    let (from, to) = ((from as usize).min(units.len()), (to as usize).min(units.len()));
    String::from_utf16_lossy(&units[from..to.max(from)])
}

fn check(dir: &Path, package: &Path, file: &Path, word: Option<&str>) -> Result<(), String> {
    let host = PluginHost::new(dir.to_path_buf(), Arc::new(|_: Option<&str>, _| {}), Limits::default())
        .map_err(|e| e.to_string())?;
    let unpacked = host.unpack_package(package).map_err(|e| e.to_string())?;
    let installed = host.install_from_dir(&unpacked, &Consent::all()).map_err(|e| e.to_string())?;
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?.replace("\r\n", "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    let doc = ProviderDocument { path: file.to_path_buf(), language: "lua".into(), version: 1, text: text.clone() };

    // The first call starts the plugin and parses the file: the second one is what an edit costs.
    let start = Instant::now();
    let first = host.provide_semantic_tokens(&doc).len();
    let cold = start.elapsed();
    let start = Instant::now();
    let tokens = host.provide_semantic_tokens(&doc);
    println!("tokens: {} ({first} on the first call, {cold:?}; then {:?})", tokens.len(), start.elapsed());
    let mut styles: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for token in &tokens {
        let style = std::iter::once(token.kind.as_str()).chain(token.modifiers.iter().map(String::as_str));
        let entry = styles.entry(style.collect::<Vec<_>>().join(" ")).or_default();
        entry.0 += 1;
        if entry.1.is_empty() {
            entry.1 = slice(&lines, token.range.start.line, token.range.start.col, token.range.end.col);
        }
    }
    for (style, (count, example)) in &styles {
        println!("  {style}: {count}, first `{example}`");
    }

    let start = Instant::now();
    let folds = host.language_folding(&doc);
    let listed: Vec<String> =
        folds.iter().map(|f| format!("{}..{} {}", f.start_line + 1, f.end_line + 1, f.kind)).collect();
    println!("folds: {} ({:?}): {}", folds.len(), start.elapsed(), listed.join(", "));

    let start = Instant::now();
    let symbols = host.language_document_symbols(&doc);
    println!("structure: {} ({:?})", symbols.len(), start.elapsed());
    for symbol in &symbols {
        let mut depth = 0;
        let mut parent = symbol.parent;
        while let Some(index) = parent {
            depth += 1;
            parent = symbols.get(index as usize).and_then(|s| s.parent);
        }
        println!("  {}{}{} {}", "  ".repeat(depth), symbol.name, symbol.detail, symbol.kind);
    }

    if let Some(word) = word {
        let at = lines.iter().enumerate().find_map(|(line, text)| {
            text.find(word).map(|byte| Pos::new(line as u32, text[..byte].encode_utf16().count() as u32))
        });
        let at = at.ok_or_else(|| format!("`{word}` is not in the file"))?;
        let start = Instant::now();
        let highlights = host.language_document_highlights(&doc, at);
        let listed: Vec<String> = highlights
            .iter()
            .map(|h| format!("{}:{} {}", h.range.start.line + 1, h.range.start.col + 1, h.kind))
            .collect();
        println!("occurrences of `{word}`: {} ({:?}): {}", highlights.len(), start.elapsed(), listed.join(", "));
    }

    let info = host.info_of(&installed.id).map_err(|e| e.to_string())?;
    println!("{} {}: {:?}", info.id, info.version, info.status);
    if tokens.is_empty() { Err("no semantic token came back".to_owned()) } else { Ok(()) }
}
