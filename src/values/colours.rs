use crate::feature::Ctx;
use crate::perf::ast::{self, Node};
use crate::perf::classify::lua_number;
use crate::values::sites::{self, Scope};

/// The host keeps this many colours per call and drops the rest.
pub const MAX_COLOURS: usize = 10_000;

#[derive(Clone, Debug, PartialEq)]
pub struct Colour {
    pub start: usize,
    pub end: usize,
    pub rgba: [f32; 4],
}

struct Literal {
    start: usize,
    end: usize,
    value: u8,
}

struct Found {
    floats: bool,
    literals: Vec<Literal>,
}

impl Found {
    fn start(&self) -> usize {
        self.literals.first().map_or(0, |l| l.start)
    }

    fn end(&self) -> usize {
        self.literals.last().map_or(0, |l| l.end)
    }

    fn colour(&self) -> Colour {
        let rgba = [0, 1, 2, 3].map(|i| f32::from(self.literals.get(i).map_or(255, |l| l.value)) / 255.0);
        Colour { start: self.start(), end: self.end(), rgba }
    }
}

pub fn colours(cx: &Ctx) -> Vec<Colour> {
    let tree = ast::parse(&cx.doc.src);
    found(&Scope::new(cx, &tree)).iter().map(Found::colour).collect()
}

/// The text replacing `start..end`, a range given by [`colours`]: only its literals change.
pub fn presentation(cx: &Ctx, start: usize, end: usize, rgba: [f32; 4]) -> Option<String> {
    let tree = ast::parse(&cx.doc.src);
    let found = found(&Scope::new(cx, &tree));
    let colour = found.iter().find(|f| f.start() == start && f.end() == end)?;
    let text = &cx.doc.src.text;
    let mut out = String::new();
    let mut at = start;
    for (i, literal) in colour.literals.iter().enumerate() {
        let value = byte(rgba[i])?;
        out.push_str(text.get(at..literal.start)?);
        if i == 3 && value == literal.value {
            out.push_str(text.get(literal.start..literal.end)?);
        } else {
            out.push_str(&format(value, colour.floats));
        }
        at = literal.end;
    }
    Some(out)
}

/// `255` for integer components, `0.502` (three decimals at most) for float ones.
pub fn format(value: u8, floats: bool) -> String {
    if !floats {
        return value.to_string();
    }
    let text = format!("{:.3}", f64::from(value) / 255.0);
    let text = text.trim_end_matches('0');
    if text.ends_with('.') { format!("{text}0") } else { text.to_owned() }
}

fn byte(channel: f32) -> Option<u8> {
    rounded(f64::from(channel) * 255.0)
}

fn rounded(value: f64) -> Option<u8> {
    let value = value.round();
    (value.is_finite() && (0.0..=255.0).contains(&value)).then_some(value as u8)
}

fn found(scope: &Scope) -> Vec<Found> {
    let ast = scope.ast;
    let mut out = Vec::new();
    for call in 0..ast.exprs.len() {
        let Some(value_call) = scope.call_at(call) else { continue };
        let args = ast.call_args(call);
        for params in &value_call.roles.colours {
            if let Some(f) = colour_of(scope, args, params) {
                out.push(f);
            }
        }
    }
    out.sort_by_key(Found::start);
    out.truncate(MAX_COLOURS);
    out
}

fn colour_of(scope: &Scope, args: &[usize], params: &sites::Colour) -> Option<Found> {
    let component = |i: usize| literal(scope, *args.get(params.red + i)?, params.floats);
    let mut literals = vec![component(0)??, component(1)??, component(2)??];
    if params.alpha
        && let Some(alpha) = component(3)
    {
        literals.push(alpha?);
    }
    Some(Found { floats: params.floats, literals })
}

/// `None` when the argument is not a number literal, `Some(None)` when it is one outside of 0 to 255.
fn literal(scope: &Scope, arg: usize, floats: bool) -> Option<Option<Literal>> {
    let ast = scope.ast;
    if !ast.is_number(arg) {
        return None;
    }
    let value = lua_number(ast.text(scope.src(), Node::Expr(arg)))?;
    let (start, end) = ast.span(Node::Expr(arg));
    let value = rounded(if floats { value * 255.0 } else { value });
    Some(value.map(|value| Literal { start, end, value }))
}

#[cfg(test)]
#[path = "colours_tests.rs"]
mod tests;
