//! Coordinate expressions of the source — `vector3(…)`, `{x = …, y = …, z = …}`,
//! `{1.0, 2.0, 3.0}` — and the number helpers of the conversions.

use crate::lua::Source;
use crate::perf::ast::{Ast, ExprKind, FieldKey, Node, StatKind};
use crate::perf::classify::lua_number;

/// How a coordinate is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    /// `vector3(x, y, z)`, `vec4(x, y, z, w)`, `vector(x, y, z)`.
    Vector,
    /// `{x = 1.0, y = 2.0, z = 3.0}`, with an optional `w`, `h` or `heading` field.
    Keyed,
    /// `{1.0, 2.0, 3.0}`: number literals only.
    Array,
}

/// A coordinate expression and its components in x, y, z (, heading) order.
#[derive(Clone, Debug)]
pub struct Coord {
    pub expr: usize,
    pub form: Form,
    pub components: Vec<usize>,
    /// The constructor called, for vectors.
    pub constructor: Option<String>,
}

impl Coord {
    pub fn texts<'s>(&self, ast: &Ast, src: &'s Source) -> Vec<&'s str> {
        self.components.iter().map(|&c| ast.text(src, Node::Expr(c))).collect()
    }

    /// The number literal (sign included) of each component, `None` for computed ones.
    pub fn number_texts(&self, ast: &Ast, src: &Source) -> Vec<Option<String>> {
        self.components.iter().map(|&c| ast.number_text(src, c)).collect()
    }

    /// The value of every component, `None` when one is computed.
    pub fn values(&self, ast: &Ast, src: &Source) -> Option<Vec<f64>> {
        self.number_texts(ast, src).into_iter().map(|t| parse_number(&t?)).collect()
    }
}

/// `e` as a coordinate expression.
pub fn analyze(ast: &Ast, src: &Source, e: usize) -> Option<Coord> {
    match &ast.exprs[e].kind {
        ExprKind::Call { callee, method: None, args } => {
            let name = ast.name_of(*callee)?;
            // Parenthesised arguments only (not `vector3 { … }`).
            if !src.is_op(ast.exprs[*callee].end, "(") {
                return None;
            }
            let ok = match name {
                "vector3" | "vec3" => args.len() == 3,
                "vector4" | "vec4" => args.len() == 4,
                // `vector(…)` sizes itself from its arguments, which may be vectors: only literal numbers are known.
                "vector" | "vec" => {
                    (3..=4).contains(&args.len()) && args.iter().all(|&a| ast.number_text(src, a).is_some())
                }
                _ => false,
            };
            ok.then_some(Coord {
                expr: e,
                form: Form::Vector,
                components: args.clone(),
                constructor: Some(name.to_owned()),
            })
        }
        ExprKind::Table { fields } => {
            if !(3..=4).contains(&fields.len())
                || fields.iter().any(|f| matches!(f.key, FieldKey::Set) || f.value.is_none())
            {
                return None;
            }
            let values: Vec<usize> = fields.iter().filter_map(|f| f.value).collect();
            if fields.iter().all(|f| matches!(f.key, FieldKey::Positional)) {
                let numbers = values.iter().all(|&v| ast.number_text(src, v).is_some());
                return numbers.then_some(Coord { expr: e, form: Form::Array, components: values, constructor: None });
            }
            let mut by_key: Vec<(String, usize)> = Vec::new();
            let mut heading = None;
            for f in fields {
                let key = field_name(ast, src, &f.key)?;
                if by_key.iter().any(|(k, _)| *k == key) {
                    return None;
                }
                match key.as_str() {
                    "x" | "y" | "z" => {}
                    "w" | "h" | "heading" if heading.is_none() => heading = Some(key.clone()),
                    _ => return None,
                }
                by_key.push((key, f.value?));
            }
            let get = |k: &str| by_key.iter().find(|(key, _)| key == k).map(|(_, v)| *v);
            let mut components = vec![get("x")?, get("y")?, get("z")?];
            components.extend(heading.and_then(|h| get(&h)));
            Some(Coord { expr: e, form: Form::Keyed, components, constructor: None })
        }
        _ => None,
    }
}

/// The name of a table field: `name = …` or `["name"] = …`.
fn field_name(ast: &Ast, src: &Source, key: &FieldKey) -> Option<String> {
    match key {
        FieldKey::Named(n) => Some(n.clone()),
        FieldKey::Expr(k) if matches!(ast.exprs[*k].kind, ExprKind::Str) => {
            Some(src.toks[ast.exprs[*k].first].string_value(&src.text))
        }
        _ => None,
    }
}

/// The innermost coordinate expression around byte `at` (or just before it), up to the enclosing statement.
pub fn find_at(ast: &Ast, src: &Source, at: usize) -> Option<Coord> {
    nodes_at(ast, src, at).into_iter().find_map(|start| {
        let mut n = Some(start);
        while let Some(node) = n {
            match node {
                Node::Stat(_) => return None,
                Node::Expr(e) => {
                    if let Some(c) = analyze(ast, src, e) {
                        return Some(c);
                    }
                }
                Node::Block(_) => {}
            }
            n = ast.parent(node);
        }
        None
    })
}

/// The nodes holding byte `at` and byte `at - 1` (a caret right after `)` or `}`).
pub fn nodes_at(ast: &Ast, src: &Source, at: usize) -> Vec<Node> {
    let mut out = Vec::new();
    for a in [Some(at), at.checked_sub(1)].into_iter().flatten() {
        let i = src.toks.partition_point(|t| t.end <= a);
        let node = match src.toks.get(i) {
            Some(t) if t.start <= a => ast.owner_of(i),
            // Between tokens: the innermost node spanning the position.
            _ => i.checked_sub(1).and_then(|p| ast.owner_of(p)).and_then(|mut n| {
                loop {
                    let (s, e) = ast.span(n);
                    if s < a && a < e {
                        break Some(n);
                    }
                    n = ast.parent(n)?;
                }
            }),
        };
        if let Some(n) = node
            && !out.contains(&n)
        {
            out.push(n);
        }
    }
    out
}

/// The name `e` is stored under: its table key, or the local / variable / field it is assigned to.
pub fn name_context(ast: &Ast, src: &Source, e: usize) -> Option<String> {
    match ast.parent(Node::Expr(e))? {
        Node::Expr(t) => match &ast.exprs[t].kind {
            ExprKind::Table { fields } => {
                fields.iter().find(|f| f.value == Some(e)).and_then(|f| field_name(ast, src, &f.key))
            }
            _ => None,
        },
        Node::Stat(s) => match &ast.stats[s].kind {
            StatKind::Local { names, values } => {
                let i = values.iter().position(|&v| v == e)?;
                names.get(i).map(|&d| ast.decls[d].name.clone())
            }
            StatKind::Assign { targets, values } => {
                let i = values.iter().position(|&v| v == e)?;
                match &ast.exprs[*targets.get(i)?].kind {
                    ExprKind::Name { name, .. } | ExprKind::Field { name, .. } => Some(name.clone()),
                    _ => None,
                }
            }
            _ => None,
        },
        Node::Block(_) => None,
    }
}

/// Value of a (negated) Lua number literal text.
pub fn parse_number(text: &str) -> Option<f64> {
    match text.strip_prefix('-') {
        Some(body) => lua_number(body).map(|v| -v),
        None => lua_number(text),
    }
}

/// Decimals of a decimal literal (`-1.234` has 3); `None` for exponents and hexadecimal numbers.
pub fn decimals(text: &str) -> Option<usize> {
    if text.contains(['e', 'E', 'x', 'X']) {
        return None;
    }
    Some(text.find('.').map_or(0, |d| text.len() - d - 1))
}

/// A decimal literal rounded half up to `decimals`, keeping one decimal: `12.3456` → `12.35`, `7.96` → `8.0`.
pub fn round(text: &str, decimals: usize) -> String {
    let (negative, body) = match text.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, text),
    };
    let (int, frac) = body.split_once('.').unwrap_or((body, ""));
    let kept = &frac[..decimals.min(frac.len())];
    let mut digits: Vec<u8> = format!("{int}{kept}{}", "0".repeat(decimals - kept.len())).into_bytes();
    if frac.as_bytes().get(decimals).is_some_and(|&d| d >= b'5') {
        let mut i = digits.len();
        loop {
            if i == 0 {
                digits.insert(0, b'1');
                break;
            }
            i -= 1;
            if digits[i] == b'9' {
                digits[i] = b'0';
            } else {
                digits[i] += 1;
                break;
            }
        }
    }
    let split = digits.len() - decimals;
    let int = String::from_utf8_lossy(&digits[..split]).trim_start_matches('0').to_owned();
    let frac = String::from_utf8_lossy(&digits[split..]).trim_end_matches('0').to_owned();
    if int.is_empty() && frac.is_empty() {
        return "0.0".into();
    }
    let int = if int.is_empty() { "0".to_owned() } else { int };
    let sign = if negative { "-" } else { "" };
    if frac.is_empty() { format!("{sign}{int}.0") } else { format!("{sign}{int}.{frac}") }
}

/// The JSON spelling of a decimal Lua literal (plain notation, `0.0` for zero); `None` for hexadecimal numbers.
pub fn json_number(text: &str) -> Option<String> {
    if text.contains(['x', 'X']) {
        return None;
    }
    let (negative, body) = match text.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (mantissa, exponent) = match body.find(['e', 'E']) {
        Some(i) => (&body[..i], body[i + 1..].parse::<i64>().ok()?),
        None => (body, 0),
    };
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if (int.is_empty() && frac.is_empty()) || !int.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit()) {
        return None;
    }
    let digits = format!("{int}{frac}");
    let scale = frac.len() as i64 - exponent;
    let plain = if scale <= 0 {
        format!("{digits}{}", "0".repeat((-scale) as usize))
    } else {
        let scale = scale as usize;
        if digits.len() > scale {
            format!("{}.{}", &digits[..digits.len() - scale], &digits[digits.len() - scale..])
        } else {
            format!("0.{}{digits}", "0".repeat(scale - digits.len()))
        }
    };
    if plain.bytes().all(|b| b == b'0' || b == b'.') {
        return Some("0.0".into());
    }
    let (i, f) = plain.split_once('.').unwrap_or((&plain, ""));
    let i = i.trim_start_matches('0');
    let i = if i.is_empty() { "0" } else { i };
    let sign = if negative { "-" } else { "" };
    Some(if f.is_empty() { format!("{sign}{i}") } else { format!("{sign}{i}.{f}") })
}

/// The constructor holding `size` components in the family of `original` (`vec3` → `vec4`).
pub fn constructor_for(original: Option<&str>, size: usize) -> String {
    match original {
        Some("vec3" | "vec4") => format!("vec{size}"),
        Some(o @ ("vector" | "vec")) => o.to_owned(),
        _ => format!("vector{size}"),
    }
}

/// Whether parameters `i`, `i + 1`, `i + 2` are x, y and z of a point: their names only differ by the axis letter.
pub fn is_xyz_at(names: &[String], i: usize) -> bool {
    let (Some(a), Some(b), Some(c)) = (names.get(i), names.get(i + 1), names.get(i + 2)) else { return false };
    let (a, b, c) = (a.as_bytes(), b.as_bytes(), c.as_bytes());
    if a.is_empty() || a.len() != b.len() || b.len() != c.len() {
        return false;
    }
    let mut differences = (0..a.len()).filter(|&k| a[k] != b[k] || a[k] != c[k]);
    let (Some(k), None) = (differences.next(), differences.next()) else { return false };
    a[k].eq_ignore_ascii_case(&b'x') && b[k].eq_ignore_ascii_case(&b'y') && c[k].eq_ignore_ascii_case(&b'z')
}

/// Whether a parameter named `name` takes a heading.
pub fn is_heading_name(name: Option<&String>) -> bool {
    name.is_some_and(|n| ["heading", "h", "w", "yaw"].contains(&n.to_ascii_lowercase().as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perf::ast::parse;

    fn coord_of(text: &str) -> Option<(Form, Vec<String>)> {
        let src = Source::new(&format!("local c = {text}"));
        let ast = parse(&src);
        let StatKind::Local { values, .. } = &ast.stats[ast.blocks[ast.root].stats[0]].kind else { return None };
        let c = analyze(&ast, &src, values[0])?;
        Some((c.form, c.texts(&ast, &src).into_iter().map(str::to_owned).collect()))
    }

    #[test]
    fn forms() {
        assert_eq!(coord_of("{x = 1.5, y = -2.5, z = 3.5}").map(|c| c.0), Some(Form::Keyed));
        assert_eq!(
            coord_of("{x = 1, y = 2, z = 3, heading = 4}").map(|c| c.1),
            Some(vec!["1".into(), "2".into(), "3".into(), "4".into()])
        );
        assert_eq!(
            coord_of("{ [\"z\"] = 3, x = 1, y = 2 }").map(|c| c.1),
            Some(vec!["1".into(), "2".into(), "3".into()])
        );
        assert_eq!(coord_of("{1.5, 2.5, 3.5, 90.0}").map(|c| c.0), Some(Form::Array));
        assert_eq!(coord_of("vec4(1.0, 2.0, 3.0, 4.0)").map(|c| c.0), Some(Form::Vector));
        assert_eq!(coord_of("vector3(a, b, c)").map(|c| c.0), Some(Form::Vector));
        assert_eq!(coord_of("vector(1.0, -2.0, 3.0)").map(|c| c.0), Some(Form::Vector));
        for not in [
            "{a, b, c}",
            "{x = 1, y = 2}",
            "{x = 1, y = 2, z = 3, name = 4}",
            "vector2(1.0, 2.0)",
            "vector3(v)",
            "vector(1.0, v)",
            "print(1.0, 2.0, 3.0)",
            "{x = 1, y = 2, z = 3, w = 4, h = 5}",
            "{ .x, .y, .z }",
        ] {
            assert!(coord_of(not).is_none(), "{not}");
        }
    }

    #[test]
    fn numbers() {
        assert_eq!(decimals("-1.234"), Some(3));
        assert_eq!(decimals("12"), Some(0));
        assert_eq!(decimals("1e3"), None);
        assert_eq!(round("12.3456", 2), "12.35");
        assert_eq!(round("7.96", 1), "8.0");
        assert_eq!(round("-0.001", 2), "0.0");
        assert_eq!(round("-99.999", 2), "-100.0");
        assert_eq!(round("-268.41234", 2), "-268.41");
        assert_eq!(round("-957.305", 2), "-957.31");
        assert_eq!(round("1.6", 0), "2.0");
        assert_eq!(round("-0.4", 0), "0.0");
        assert_eq!(json_number(".5").as_deref(), Some("0.5"));
        assert_eq!(json_number("5.").as_deref(), Some("5"));
        assert_eq!(json_number("-1.5").as_deref(), Some("-1.5"));
        assert_eq!(json_number("90.0").as_deref(), Some("90.0"));
        assert_eq!(json_number("-0.0").as_deref(), Some("0.0"));
        assert_eq!(json_number("1e3").as_deref(), Some("1000"));
        assert_eq!(json_number("1.5e-2").as_deref(), Some("0.015"));
        assert_eq!(json_number("0x10"), None);
        assert_eq!(parse_number("-0x10"), Some(-16.0));
        assert_eq!(constructor_for(Some("vec3"), 4), "vec4");
        assert_eq!(constructor_for(Some("vector"), 4), "vector");
        assert_eq!(constructor_for(Some("vector3"), 4), "vector4");
    }

    #[test]
    fn xyz_parameters() {
        let names = |s: &str| s.split(',').map(str::to_owned).collect::<Vec<_>>();
        assert!(is_xyz_at(&names("entity,xPos,yPos,zPos"), 1));
        assert!(is_xyz_at(&names("posX,posY,posZ"), 0));
        assert!(is_xyz_at(&names("x1,y1,z1"), 0));
        assert!(!is_xyz_at(&names("entity,xPos,yPos,zPos"), 0));
        assert!(!is_xyz_at(&names("target"), 0));
        assert!(is_heading_name(Some(&"Heading".to_owned())));
    }
}
