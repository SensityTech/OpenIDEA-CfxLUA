//! The insert actions: the text of the original's live templates with every
//! placeholder at its default, put at the caret of the window's active editor.

use super::data::{BuiltinEvent, Directive, SyntaxFeature};
use crate::natives::Native;
use openidea_plugin::{Error, Invocation, TextEdit};

/// `local retval, groundZ = GetGroundZFor_3dCoord(x, y, z, ignoreWater)` (varargs left out).
pub fn native_call(n: &Native) -> String {
    let args: Vec<&str> = n.lua_params().filter(|p| !p.is_vararg()).map(|p| p.name.as_str()).collect();
    let call = format!("{}({})", n.lua_name, args.join(", "));
    if n.returns.is_empty() {
        return call;
    }
    let returns: Vec<&str> = n.returns.iter().map(|r| r.name.as_str()).collect();
    format!("local {} = {call}", returns.join(", "))
}

/// `AddEventHandler('name', function(params)` + an indented empty line + `end)`,
/// later lines indented like the caret's line; `RegisterNetEvent` for the network variant.
pub fn handler(e: &BuiltinEvent, network: bool, indent: &str) -> String {
    let function = if network { "RegisterNetEvent" } else { "AddEventHandler" };
    let params: Vec<&str> = e.params.iter().map(|p| p.name.as_str()).collect();
    format!("{function}('{}', function({})\n{indent}    \n{indent}end)", e.name, params.join(", "))
}

/// The directive's example, else `name {\n}` (lists) / `name ''`.
pub fn directive(d: &Directive) -> String {
    if !d.example.trim().is_empty() {
        d.example.clone()
    } else if d.plural {
        format!("{} {{\n}}", d.name)
    } else {
        format!("{} ''", d.name)
    }
}

pub fn example(f: &SyntaxFeature) -> String {
    format!("{}\n", f.example)
}

/// Replaces the selection (or inserts at the caret) of the window's active
/// editor with `text(indentation of the caret's line)`, retrying once if the
/// document changed meanwhile. The file name on success, else what to tell the user.
pub fn at_caret(inv: &Invocation, text: impl Fn(&str) -> String) -> Result<String, String> {
    let editor = inv.editor().ok_or_else(|| "Open a file in the editor first: the text is inserted at its caret.".to_owned())?;
    let failed = |e: Error| format!("The text could not be inserted: {e}");
    let mut last = Error::Stale;
    for _ in 0..2 {
        let doc = editor.document().map_err(failed)?;
        let range = editor.selections().map_err(failed)?.first().copied().unwrap_or_default();
        let line = doc.text.split('\n').nth(range.start.line as usize).unwrap_or("");
        let indent: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        match editor.apply_edits(doc.version, &[TextEdit { range, text: text(&indent) }]) {
            Ok(_) => return Ok(crate::text::file_name(&crate::text::slash(&editor.path())).to_owned()),
            Err(Error::Stale) => last = Error::Stale,
            Err(e) => return Err(failed(e)),
        }
    }
    Err(failed(last))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docs::data;
    use crate::docs::search::tests::fixture;
    use openidea_plugin::mock::MockHost;

    #[test]
    fn template_texts() {
        let natives = fixture();
        let ground = natives.iter().find(|n| n.lua_name == "GetGroundZFor_3dCoord").unwrap();
        assert_eq!(native_call(ground), "local retval, groundZ = GetGroundZFor_3dCoord(x, y, z, ignoreWater)");
        let drop = natives.iter().find(|n| n.lua_name == "DropPlayer").unwrap();
        assert_eq!(native_call(drop), "DropPlayer(playerSrc, reason)");
        let start = data::event("onResourceStart").unwrap();
        assert_eq!(handler(start, false, ""), "AddEventHandler('onResourceStart', function(resourceName)\n    \nend)");
        assert_eq!(handler(start, true, "  "), "RegisterNetEvent('onResourceStart', function(resourceName)\n      \n  end)");
        assert_eq!(directive(data::directive("fx_version").unwrap()), "fx_version 'cerulean'");
        let f = data::syntax_features().iter().find(|f| f.id == "compound-assignment").unwrap();
        assert!(example(f).starts_with("local count = 10\n") && example(f).ends_with("stats.kills += 1\n"));
    }

    #[test]
    fn inserts_at_the_caret_with_its_indentation() {
        let mock = MockHost::install();
        mock.open_editor("/project/res/client.lua", "lua", "CreateThread(function()\n    x\nend)", &[(1, 4, 1, 5)]);
        let start = data::event("onResourceStart").unwrap();
        let file = at_caret(&mock.invocation(), |indent| handler(start, false, indent)).unwrap();
        assert_eq!(file, "client.lua");
        assert_eq!(
            mock.editor_text(),
            "CreateThread(function()\n    AddEventHandler('onResourceStart', function(resourceName)\n        \n    end)\nend)"
        );
        let denied = MockHost::install().with_capabilities(&["editor.read"]);
        denied.open_editor("/project/a.lua", "lua", "", &[(0, 0, 0, 0)]);
        assert!(at_caret(&denied.invocation(), |_| "x".into()).unwrap_err().starts_with("The text could not be inserted"));
        let none = MockHost::install();
        assert!(at_caret(&none.invocation(), |_| "x".into()).unwrap_err().starts_with("Open a file"));
    }
}
