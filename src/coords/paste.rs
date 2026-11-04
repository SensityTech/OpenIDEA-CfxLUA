use super::expr::{is_heading_name, is_xyz_at, json_number};
use super::text::{self, CoordText, Format};
use crate::lua::Source;
use crate::lua::lexer::Tok;
use crate::perf::ast::{Ast, ExprKind, Node, StatKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Site {
    Value,
    TableEntry,
    ValueList,
    Argument { call: usize, index: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Vector,
    Keyed,
    Array,
    List,
    Json,
}

impl Target {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "vector" => Self::Vector,
            "keyed" => Self::Keyed,
            "array" => Self::Array,
            "list" => Self::List,
            "json" => Self::Json,
            _ => return None,
        })
    }

    fn render(self, c: &CoordText) -> Option<String> {
        Some(match self {
            Self::Vector => c.to_vector(),
            Self::Keyed => text::keyed(&c.components),
            Self::Array => text::array(&c.components),
            Self::List => text::join(&c.components),
            Self::Json => {
                let fields: Option<Vec<String>> = c
                    .components
                    .iter()
                    .zip(text::KEYS)
                    .map(|(v, k)| Some(format!("\"{k}\": {}", json_number(v)?)))
                    .collect();
                format!("{{{}}}", fields?.join(", "))
            }
        })
    }
}

pub const MAX_TEXT_LENGTH: usize = 20_000;
const MAX_LINES: usize = 500;

struct Piece<'a> {
    leading: &'a str,
    body: &'a str,
    separator: &'a str,
    trailing: &'a str,
}

fn split(line: &str) -> Piece<'_> {
    let end = line.trim_end();
    let trailing = &line[end.len()..];
    let body_start = end.len() - end.trim_start().len();
    let leading = &end[..body_start];
    let mut body = &end[body_start..];
    let mut separator = "";
    if body.ends_with([',', ';']) {
        separator = &body[body.len() - 1..];
        body = body[..body.len() - 1].trim_end();
    }
    Piece { leading, body, separator, trailing }
}

pub fn convert(
    text: &str,
    site: Option<Site>,
    target: Option<Target>,
    params: &dyn Fn(usize) -> Option<Vec<String>>,
) -> Option<String> {
    if text.len() > MAX_TEXT_LENGTH {
        return None;
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let parsed: Vec<usize> = (0..lines.len()).filter(|&i| !lines[i].trim().is_empty()).collect();
    if parsed.is_empty() || parsed.len() > MAX_LINES {
        return None;
    }
    let pieces: Vec<Piece> = parsed.iter().map(|&i| split(lines[i])).collect();
    let coords: Vec<CoordText> = pieces.iter().map(|p| text::parse(p.body)).collect::<Option<_>>()?;
    let site = site.unwrap_or(Site::Value);
    if coords.len() > 1 && site != Site::TableEntry && target.is_none() {
        return None;
    }
    let convert = |c: &CoordText| -> Option<String> {
        if let Some(t) = target {
            return t.render(c);
        }
        let vector = || (!c.lua_vector).then(|| c.to_vector());
        let unless_list = || (!c.lua_vector && c.format != Format::List).then(|| c.to_vector());
        match site {
            Site::Value | Site::TableEntry => vector(),
            Site::ValueList => unless_list(),
            Site::Argument { call, index } => match params(call) {
                Some(names) if is_xyz_at(&names, index) => {
                    let size = if is_heading_name(names.get(index + 3)) { 4 } else { 3 };
                    // Numbers pasted as numbers are already right.
                    (c.format != Format::List).then(|| text::join(&c.components[..size.min(c.components.len())]))
                }
                _ => unless_list(),
            },
        }
    };
    let mut out: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
    let mut changed = false;
    let last = parsed.last().copied();
    for ((&line, piece), coord) in parsed.iter().zip(&pieces).zip(&coords) {
        let converted = convert(coord);
        changed |= converted.is_some();
        // Entries pasted together need separators.
        let separator = if piece.separator.is_empty() && Some(line) != last { "," } else { piece.separator };
        changed |= separator != piece.separator;
        out[line] =
            format!("{}{}{separator}{}", piece.leading, converted.as_deref().unwrap_or(piece.body), piece.trailing);
    }
    changed.then(|| out.join("\n"))
}

pub fn site(ast: &Ast, src: &Source, at: usize) -> Option<Site> {
    let i = src.toks.partition_point(|t| t.end <= at).checked_sub(1)?;
    let tok = src.toks[i];
    let t = tok.text(&src.text);
    let owner = ast.owner_of(i);
    let expr_kind = |n: Option<Node>| match n {
        Some(Node::Expr(e)) => Some(&ast.exprs[e].kind),
        _ => None,
    };
    let stat_kind = |n: Option<Node>| match n {
        Some(Node::Stat(s)) => Some(&ast.stats[s].kind),
        _ => None,
    };
    // After the `=` of a statement (not in its name or target list).
    let after_assign = |s: usize| ast.stats[s].first < i && (ast.stats[s].first..i).any(|k| src.is_op(k, "="));
    match (tok.kind, t) {
        (Tok::Op, "=") => match (expr_kind(owner), stat_kind(owner)) {
            (Some(ExprKind::Table { .. }), _) => Some(Site::Value),
            (_, Some(StatKind::Local { names, .. })) => {
                Some(if names.len() > 1 { Site::ValueList } else { Site::Value })
            }
            (_, Some(StatKind::Assign { targets, .. })) => {
                Some(if targets.len() > 1 { Site::ValueList } else { Site::Value })
            }
            _ => None,
        },
        (Tok::Op, "+=" | "-=" | "*=" | "/=" | "%=" | "^=" | "..=" | "//=" | "<<=" | ">>=" | "&=" | "|=") => {
            Some(Site::Value)
        }
        (Tok::Op, "," | ";") => match (owner, expr_kind(owner)) {
            (_, Some(ExprKind::Table { .. })) => Some(Site::TableEntry),
            (Some(Node::Expr(call)), Some(ExprKind::Call { .. })) if t == "," => Some(argument(ast, src, call, at)),
            (Some(Node::Stat(s)), _) if t == "," => match &ast.stats[s].kind {
                StatKind::Return(_) | StatKind::GenFor { .. } => Some(Site::ValueList),
                StatKind::Local { .. } | StatKind::Assign { .. } if after_assign(s) => Some(Site::ValueList),
                _ => None,
            },
            _ => None,
        },
        (Tok::Op, "{") => matches!(expr_kind(owner), Some(ExprKind::Table { .. })).then_some(Site::TableEntry),
        (Tok::Op, "(") => match (owner, expr_kind(owner)) {
            (Some(Node::Expr(call)), Some(ExprKind::Call { .. })) => Some(argument(ast, src, call, at)),
            (_, Some(ExprKind::Paren(_))) => Some(Site::Value),
            _ => None,
        },
        (Tok::Keyword, "return") => Some(Site::ValueList),
        _ => matches!(expr_kind(owner), Some(ExprKind::Binary { .. } | ExprKind::Unary { .. })).then_some(Site::Value),
    }
}

fn argument(ast: &Ast, src: &Source, call: usize, at: usize) -> Site {
    let e = &ast.exprs[call];
    let index = (e.first..e.end)
        .filter(|&k| src.toks[k].end <= at && src.is_op(k, ",") && ast.owner_of(k) == Some(Node::Expr(call)))
        .count();
    Site::Argument { call, index }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perf::ast::parse;

    fn paste(before: &str, clipboard: &str) -> String {
        let at = before.find("<caret>").unwrap();
        let doc = before.replace("<caret>", "");
        let src = Source::new(&doc);
        let ast = parse(&src);
        let params = |call: usize| -> Option<Vec<String>> {
            let names: &[&str] = match ast.callee_name(call)? {
                "SetEntityCoords" => {
                    &["entity", "xPos", "yPos", "zPos", "alive", "deadFlag", "ragdollFlag", "clearArea"]
                }
                "CreatePed" => &["pedType", "modelHash", "x", "y", "z", "heading", "isNetwork", "bScriptHostPed"],
                "TeleportTo" => &["target"],
                _ => return None,
            };
            Some(names.iter().map(|s| (*s).to_owned()).collect())
        };
        let found = site(&ast, &src, at);
        let pasted = convert(clipboard, found, None, &params).unwrap_or_else(|| clipboard.to_owned());
        format!("{}{pasted}{}", &doc[..at], &doc[at..])
    }

    fn unchanged(before: &str, clipboard: &str) {
        assert_eq!(paste(before, clipboard), before.replace("<caret>", clipboard), "{before} + {clipboard}");
    }

    #[test]
    fn each_format_becomes_a_vector_where_a_value_is_expected() {
        for (clipboard, vector) in [
            ("123.4, 56.7, 8.9", "vector3(123.4, 56.7, 8.9)"),
            ("123.4, 56.7, 8.9, 90.0", "vector4(123.4, 56.7, 8.9, 90.0)"),
            ("Vector3(1.0, 2.0, 3.0)", "vector3(1.0, 2.0, 3.0)"),
            ("new Vector3(1.5f, 2.5f, 3.5f)", "vector3(1.5, 2.5, 3.5)"),
            ("{x = 1.0, y = 2.0, z = 3.0}", "vector3(1.0, 2.0, 3.0)"),
            ("{\"x\": 1.0, \"y\": 2.0, \"z\": 3.0, \"w\": 4.0}", "vector4(1.0, 2.0, 3.0, 4.0)"),
            ("x: 1.0 y: 2.0 z: 3.0", "vector3(1.0, 2.0, 3.0)"),
            ("X: -1.5, Y: 2.5, Z: 3.5, Heading: 180.0", "vector4(-1.5, 2.5, 3.5, 180.0)"),
        ] {
            assert_eq!(paste("local pos = <caret>", clipboard), format!("local pos = {vector}"), "{clipboard}");
        }
    }

    #[test]
    fn expression_positions() {
        assert_eq!(paste("Config.Spawn = <caret>", "1.5, 2.5, 3.5"), "Config.Spawn = vector3(1.5, 2.5, 3.5)");
        assert_eq!(
            paste("local t = { coords = <caret> }", "1.5, 2.5, 3.5"),
            "local t = { coords = vector3(1.5, 2.5, 3.5) }"
        );
        assert_eq!(paste("local t = { <caret> }", "1.5, 2.5, 3.5"), "local t = { vector3(1.5, 2.5, 3.5) }");
        assert_eq!(paste("local t = { a, <caret> }", "1.5, 2.5, 3.5"), "local t = { a, vector3(1.5, 2.5, 3.5) }");
        assert_eq!(paste("local d = #(pos - <caret>)", "1.5, 2.5, 3.5"), "local d = #(pos - vector3(1.5, 2.5, 3.5))");
        assert_eq!(paste("pos += <caret>", "1.5, 2.5, 3.5"), "pos += vector3(1.5, 2.5, 3.5)");
        unchanged("local x, y, z = <caret>", "1.5, 2.5, 3.5");
        unchanged("return <caret>", "1.5, 2.5, 3.5");
        assert_eq!(paste("return <caret>", "x: 1.5 y: 2.5 z: 3.5"), "return vector3(1.5, 2.5, 3.5)");
    }

    #[test]
    fn xyz_arguments_get_the_numbers() {
        assert_eq!(
            paste("SetEntityCoords(ped, <caret>)", "vector3(1.5, 2.5, 3.5)"),
            "SetEntityCoords(ped, 1.5, 2.5, 3.5)"
        );
        assert_eq!(
            paste("SetEntityCoords(ped, <caret>)", "{\"x\": 1.5, \"y\": 2.5, \"z\": 3.5, \"w\": 90.0}"),
            "SetEntityCoords(ped, 1.5, 2.5, 3.5)"
        );
        assert_eq!(
            paste("CreatePed(4, model, <caret>)", "vector4(1.5, 2.5, 3.5, 90.0)"),
            "CreatePed(4, model, 1.5, 2.5, 3.5, 90.0)"
        );
        unchanged("SetEntityCoords(ped, <caret>)", "1.5, 2.5, 3.5");
        assert_eq!(paste("TeleportTo(<caret>)", "x: 1.5 y: 2.5 z: 3.5"), "TeleportTo(vector3(1.5, 2.5, 3.5))");
        unchanged("TeleportTo(<caret>)", "vector3(1.5, 2.5, 3.5)");
        unchanged("print(<caret>)", "1.5, 2.5, 3.5");
        unchanged("SetEntityCoords(<caret>)", "vector3(1.5, 2.5, 3.5)");
    }

    #[test]
    fn several_coordinates_become_table_entries() {
        assert_eq!(
            paste("Config.Spots = {\n    <caret>\n}", "1.5, 2.5, 3.5\n    -4.5, 5.5, 6.5, 90.0\n"),
            "Config.Spots = {\n    vector3(1.5, 2.5, 3.5),\n    vector4(-4.5, 5.5, 6.5, 90.0)\n\n}"
        );
        assert!(!paste("Config.Spots = { <caret> }", "1.5, 2.5, 3.5\nhello").contains("vector3"));
        unchanged("local pos = <caret>", "1.5, 2.5, 3.5\n4.5, 5.5, 6.5");
    }

    #[test]
    fn statements_and_non_coordinates() {
        unchanged("local pos = <caret>", "vector3(1.0, 2.0, 3.0)");
        unchanged("local pos = <caret>", "vec4(1.0, 2.0, 3.0, 4.0)");
        for clipboard in ["1, 2, 3", "1.5, 2.5", "hello", "{1.5, 2.5, 3.5}", "{x = 1.0, y = 2.0}", "a, b, c"] {
            unchanged("local pos = <caret>", clipboard);
        }
        let src = Source::new("local a = 1\nfor i = 1\nfunction f() end");
        let ast = parse(&src);
        assert_eq!(site(&ast, &src, 0), None);
        assert_eq!(site(&ast, &src, src.text.find("for").unwrap()), None);
        assert_eq!(site(&ast, &src, src.text.find("i = ").unwrap() + 4), None, "for i = |");
        assert_eq!(site(&ast, &src, src.text.find("f()").unwrap() + 2), None, "function f(|)");
    }

    #[test]
    fn explicit_formats() {
        let none = |_: usize| None;
        let c = "vector4(-1.5, .5, 3., 90.0)";
        assert_eq!(
            convert(c, None, Some(Target::Json), &none).as_deref(),
            Some("{\"x\": -1.5, \"y\": 0.5, \"z\": 3, \"w\": 90.0}")
        );
        assert_eq!(
            convert("1.5, 2.5, 3.5", None, Some(Target::Keyed), &none).as_deref(),
            Some("{ x = 1.5, y = 2.5, z = 3.5 }")
        );
        assert_eq!(
            convert("vec3(1.0, 2.0, 3.0)", None, Some(Target::Array), &none).as_deref(),
            Some("{ 1.0, 2.0, 3.0 }")
        );
        assert_eq!(
            convert("1.5, 2.5, 3.5\n4.5, 5.5, 6.5", None, Some(Target::Vector), &none).as_deref(),
            Some("vector3(1.5, 2.5, 3.5),\nvector3(4.5, 5.5, 6.5)")
        );
        assert_eq!(Target::parse("JSON"), Some(Target::Json));
        assert_eq!(Target::parse("hex"), None);
    }
}
