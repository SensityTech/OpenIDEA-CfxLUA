//! The value of an operation between constants (`ExpressionUtil.compute`), with the truth and the modulo of
//! Lua: `nil` and `false` are the only values `and` and `or` take for false, and what is not a constant is
//! never taken for true.

use crate::coords::expr::parse_number;
use crate::lua::Source;
use crate::lua::joaat::joaat;
use crate::lua::token::T;
use crate::lua::tree::{N, Node};
use crate::ty::psi::{self, Literal};

enum Value {
    Number(f64),
    Text(String),
    Bool(bool),
    Nil,
    /// An expression that is no constant, as it is written.
    Other(String),
}

impl Value {
    /// Whether `and` and `or` take it for true: unknown for what is no constant.
    fn truth(&self) -> Option<bool> {
        match self {
            Value::Nil | Value::Bool(false) => Some(false),
            Value::Other(_) => None,
            _ => Some(true),
        }
    }
}

fn number(value: f64) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    let whole = value.fract() == 0.0 && value.abs() < 1e15;
    Some(if whole { format!("{}", value as i64) } else { format!("{value}") })
}

/// A string as a literal between the quotes `quote`.
fn quoted(text: &str, quote: char) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_control() => out.push_str(&format!("\\{}", c as u32)),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

fn of(src: &Source, expr: Node) -> Option<Value> {
    match expr.kind() {
        N::LiteralExpr => match psi::literal_kind(expr) {
            Literal::String => psi::string_value(src, expr).map(Value::Text),
            Literal::Bool => Some(Value::Bool(expr.code() == "true")),
            Literal::Number => parse_number(expr.code()).map(Value::Number),
            Literal::Hash => {
                let name = expr.code().strip_prefix('`')?.strip_suffix('`')?;
                Some(Value::Number(f64::from(joaat(name))))
            }
            Literal::Nil => Some(Value::Nil),
            _ => None,
        },
        N::BinaryExpr => {
            let mut operands = expr.exprs();
            let (left, right) = (of(src, operands.next()?)?, of(src, operands.next()?)?);
            let operator = expr.child(N::BinaryOp)?.first_token()?.kind();
            binary(left, right, operator)
        }
        N::ParenExpr => of(src, expr.expr()?),
        _ => Some(Value::Other(expr.code().to_owned())),
    }
}

fn binary(left: Value, right: Value, operator: T) -> Option<Value> {
    let text = |value: &Value| match value {
        Value::Text(text) => Some(text.clone()),
        Value::Number(value) => number(*value),
        _ => None,
    };
    match (operator, &left, &right) {
        (T::Or, _, _) => Some(if left.truth()? { left } else { right }),
        (T::And, _, _) => Some(if left.truth()? { right } else { left }),
        (T::Plus, Value::Number(a), Value::Number(b)) => Some(Value::Number(a + b)),
        (T::Minus, Value::Number(a), Value::Number(b)) => Some(Value::Number(a - b)),
        (T::Mult, Value::Number(a), Value::Number(b)) => Some(Value::Number(a * b)),
        (T::Div, Value::Number(a), Value::Number(b)) if *b != 0.0 => Some(Value::Number(a / b)),
        (T::DoubleDiv, Value::Number(a), Value::Number(b)) if *b != 0.0 => Some(Value::Number((a / b).floor())),
        (T::Mod, Value::Number(a), Value::Number(b)) if *b != 0.0 => Some(Value::Number(a - (a / b).floor() * b)),
        (T::Concat, Value::Text(_), Value::Text(_) | Value::Number(_))
        | (T::Concat, Value::Number(_), Value::Text(_)) => Some(Value::Text(text(&left)? + &text(&right)?)),
        _ => None,
    }
}

/// The quote of the first string the operation is written with.
fn quote_of(operation: Node) -> char {
    let strings = operation.descendants().filter(|node| psi::is_string(*node));
    let quotes = strings.filter_map(|literal| literal.code().chars().next()).find(|first| matches!(first, '\'' | '"'));
    quotes.unwrap_or('"')
}

/// What an operation between constants is worth, as it is written in its place.
pub fn value(src: &Source, operation: Node) -> Option<String> {
    Some(match of(src, operation)? {
        Value::Number(value) => number(value)?,
        Value::Text(text) => quoted(&text, quote_of(operation)),
        Value::Bool(value) => value.to_string(),
        Value::Nil => "nil".to_owned(),
        Value::Other(code) => code,
    })
}
