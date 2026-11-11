pub mod errors;
pub mod scope;
pub mod tree;

use crate::feature::{Ctx, Feature, Fix, Problem};
use crate::lua::Source;
use crate::lua::lexer::Tok;
use crate::project::FileKind;
use openidea_plugin::Severity;
use scope::Scopes;
use tree::{Kind, Span, Stmt, Tree, is_var};

#[derive(Default)]
pub struct SyntaxFeature;

impl Feature for SyntaxFeature {
    fn problems(&self, cx: &Ctx) -> Vec<Problem> {
        let lua = cx.doc.is_lua() || matches!(cx.doc.kind, FileKind::Lua | FileKind::Manifest);
        if !cx.settings.enabled() || !lua {
            return Vec::new();
        }
        check(&cx.doc.src)
    }
}

pub fn check(src: &Source) -> Vec<Problem> {
    let tree = Tree::new(src);
    let scopes = Scopes::new(src, &tree);
    let stmts = tree.statements();
    let mut out = Vec::new();
    for s in &stmts {
        match &s.kind {
            Kind::Compound { target, op, value } => compound(src, &scopes, s, *target, *op, *value, &mut out),
            Kind::Local { names, unpack: true, .. } => {
                for n in names {
                    if let Some((lt, gt)) = n.attr
                        && src.tok_text(lt + 1) == "close"
                    {
                        out.push(Problem::new(
                            "CfxCloseInUnpacking",
                            Severity::Warning,
                            src.toks[lt].start,
                            src.toks[gt].end,
                            "The <close> attribute is ignored in an 'in' unpacking: the value is never closed",
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    decrements(src, &scopes, &stmts, &mut out);
    increments(src, &stmts, &mut out);
    operators(src, &mut out);
    let unread = errors::errors(src, &out);
    out.extend(unread);
    out.sort_by_key(|p| p.start);
    out
}

fn text_of(src: &Source, (a, b): Span) -> &str {
    let (s, e) = src.span(a, b);
    &src.text[s..e]
}

fn compound(src: &Source, scopes: &Scopes, s: &Stmt, target: Span, op: usize, value: Span, out: &mut Vec<Problem>) {
    let op_tok = src.toks[op];
    let op_text = op_tok.text(&src.text);
    // `c += 1` on a `local c <const>` (or `<close>`).
    if target.1 == target.0 + 1
        && let Some(d) = scopes.decl(target.0)
        && let Some(attr @ ("const" | "close")) = scopes.attr(src, d)
    {
        let t = src.toks[target.0];
        out.push(Problem::new(
            "CfxConstCompoundAssign",
            Severity::Error,
            t.start,
            t.end,
            format!(
                "Compound assignment to <{attr}> variable '{}': CfxLua does not reject it but ignores or corrupts the assignment",
                t.text(&src.text)
            ),
        ));
    }
    let target_text = text_of(src, target);
    let (start, _) = src.span(s.start, s.end);
    let (_, end) = src.span(value.0, value.1);
    let has_value = value.1 > value.0;
    let value_text = text_of(src, value);
    match op_text {
        "^=" => {
            let mut p = Problem::new(
                "CfxXorAssign",
                Severity::Info,
                op_tok.start,
                op_tok.end,
                "In CfxLua '^=' is bitwise XOR, not exponentiation",
            );
            if has_value {
                p = p.fix(Fix::new("Replace with 'x = x ^ (rhs)' (exponentiation)").replace(
                    start,
                    end,
                    format!("{target_text} = {target_text} ^ ({value_text})"),
                ));
            }
            out.push(p);
        }
        "//=" | "%=" | "..=" => {
            let bin = &op_text[..op_text.len() - 1];
            let mut p = Problem::new(
                "CfxUnsupportedOperator",
                Severity::Error,
                op_tok.start,
                op_tok.end,
                format!("'{op_text}' is not supported in CfxLua, use 'x = x {bin} y'"),
            );
            if has_value {
                let v = if needs_parens(src, value) { format!("({value_text})") } else { value_text.to_owned() };
                let replacement = format!("{target_text} = {target_text} {bin} {v}");
                p = p.fix(
                    Fix::new(format!("Replace with '{replacement}'")).replace(start, end, replacement).preferred(),
                );
            }
            out.push(p);
        }
        _ => {}
    }
}

fn needs_parens(src: &Source, (a, b): Span) -> bool {
    let mut i = a;
    while i < b {
        let t = src.toks[i];
        match t.kind {
            Tok::Op => match t.text(&src.text) {
                "(" | "[" | "{" => {
                    i = src.pair[i].map_or(b, |c| c + 1);
                    continue;
                }
                "." | ":" | "?" => {}
                _ => return true,
            },
            Tok::Keyword if matches!(t.text(&src.text), "and" | "or" | "not") => return true,
            _ => {}
        }
        i += 1;
    }
    false
}

fn code_index(src: &Source, start: usize) -> Option<usize> {
    let i = src.toks.partition_point(|t| t.start < start);
    (src.toks.get(i)?.start == start).then_some(i)
}

fn decrements(src: &Source, scopes: &Scopes, stmts: &[&Stmt], out: &mut Vec<Problem>) {
    for (ai, c) in src.all.iter().enumerate() {
        if c.kind != Tok::LineComment || ai == 0 {
            continue;
        }
        let text = c.text(&src.text);
        let region = ["--region", "--endregion", "--{{{", "--}}}"].iter().any(|r| text.starts_with(r));
        if text.starts_with("---") || region {
            continue;
        }
        let p = src.all[ai - 1];
        if p.end != c.start || p.is_comment() {
            continue;
        }
        let Some(k) = code_index(src, p.start) else { continue };
        let variable = match p.kind {
            Tok::Name => {
                let declared = scopes.decl(k).is_some_and(|d| d.tok == k);
                let label = k > 0 && (src.is_kw(k - 1, "goto") || src.is_op(k - 1, "::"));
                !declared && !label
            }
            Tok::Op if p.text(&src.text) == "]" => src.pair[k].is_some_and(|o| {
                o > 0
                    && src.is_op(o, "[")
                    && (matches!(src.toks[o - 1].kind, Tok::Name | Tok::Str { .. } | Tok::LongStr)
                        || src.is_op(o - 1, ")")
                        || src.is_op(o - 1, "]")
                        || src.is_op(o - 1, "?"))
            }),
            _ => false,
        };
        if !variable {
            continue;
        }
        let mut problem = Problem::new(
            "CfxDecrementComment",
            Severity::Warning,
            c.start,
            c.start + 2,
            "CfxLua has no '--' decrement operator: this starts a comment",
        );
        let whole = stmts.iter().find_map(|s| match s.kind {
            Kind::Expr((a, b)) if b == k + 1 && is_var(src, (a, b)) => Some((a, b)),
            _ => None,
        });
        if let Some(span) = whole {
            let target = text_of(src, span);
            let start = src.toks[span.0].start;
            let fix = Fix::new(format!("Replace with '{target} -= 1'"));
            let fix = if text[2..].trim().is_empty() {
                fix.replace(start, c.end, format!("{target} -= 1"))
            } else {
                fix.replace(start, c.start, format!("{target} -= 1 "))
            };
            problem = problem.fix(fix.preferred());
        }
        out.push(problem);
    }
}

fn increments(src: &Source, stmts: &[&Stmt], out: &mut Vec<Problem>) {
    for (k, t) in src.toks.iter().enumerate() {
        if !t.is_op(&src.text, "++") {
            continue;
        }
        let mut problem = Problem::new(
            "CfxIncrement",
            Severity::Error,
            t.start,
            t.end,
            "'++' is not supported in CfxLua, use 'x += 1'",
        );
        let glued = src.toks.get(k + 1).is_some_and(|n| n.start == t.end);
        let postfix = stmts.iter().find_map(|s| match s.kind {
            Kind::Expr((a, b)) if b == k && !glued && is_var(src, (a, b)) => Some(((a, b), src.toks[a].start, t.end)),
            _ => None,
        });
        let line = src.lines.line_of(t.start);
        let prefix = || {
            stmts.iter().find_map(|s| match s.kind {
                Kind::Expr((a, b))
                    if a == k + 1 && is_var(src, (a, b)) && src.lines.line_of(src.toks[a].start) == line =>
                {
                    Some(((a, b), t.start, src.toks[b - 1].end))
                }
                _ => None,
            })
        };
        if let Some((span, start, end)) = postfix.or_else(prefix) {
            let target = text_of(src, span);
            problem = problem.fix(
                Fix::new(format!("Replace with '{target} += 1'"))
                    .replace(start, end, format!("{target} += 1"))
                    .preferred(),
            );
        }
        out.push(problem);
    }
}

fn operators(src: &Source, out: &mut Vec<Problem>) {
    let mut k = 0;
    while k < src.toks.len() {
        let t = src.toks[k];
        k += 1;
        if t.kind != Tok::Op {
            continue;
        }
        let (end, typed, lua) = match t.text(&src.text) {
            "!=" => (t.end, "!=", "~="),
            "!" => (t.end, "!", "not"),
            op @ ("&" | "|") => match src.toks.get(k) {
                Some(n) if n.start == t.end && n.text(&src.text) == op => {
                    k += 1;
                    (n.end, if op == "&" { "&&" } else { "||" }, if op == "&" { "and" } else { "or" })
                }
                _ => continue,
            },
            _ => continue,
        };
        let before = src.text[..t.start].chars().next_back().is_some_and(|c| !c.is_whitespace());
        let after = src.text[end..].chars().next().is_some_and(|c| !c.is_whitespace());
        let replacement = match typed {
            "!=" => lua.to_owned(),
            "!" => format!("{lua}{}", if after { " " } else { "" }),
            _ => format!("{}{lua}{}", if before { " " } else { "" }, if after { " " } else { "" }),
        };
        out.push(
            Problem::new(
                "CfxUnsupportedOperator",
                Severity::Error,
                t.start,
                end,
                format!("'{typed}' is not supported in CfxLua, use '{lua}'"),
            )
            .fix(Fix::new(format!("Replace with '{lua}'")).replace(t.start, end, replacement).preferred()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::lexer::tokenize;
    use crate::testing::Fixture;

    fn problems(text: &str) -> Vec<String> {
        let src = Source::new(text);
        check(&src).iter().map(|p| format!("{}: {}", &text[p.start..p.end], p.message)).collect()
    }

    fn fixed(text: &str, title: &str) -> String {
        let src = Source::new(text);
        let problems = check(&src);
        let fix = problems.iter().flat_map(|p| &p.fixes).find(|f| f.title == title);
        let fix = fix.unwrap_or_else(|| {
            panic!(
                "no fix '{title}' in {:?}",
                problems.iter().flat_map(|p| &p.fixes).map(|f| &f.title).collect::<Vec<_>>()
            )
        });
        let mut out = text.to_owned();
        let mut edits = fix.edits.clone();
        edits.sort_by_key(|e| std::cmp::Reverse(e.0));
        for (s, e, t) in edits {
            out.replace_range(s..e, &t);
        }
        out
    }

    const CONST: &str = "CfxLua does not reject it but ignores or corrupts the assignment";

    #[test]
    fn what_the_parser_cannot_read() {
        assert_eq!(problems("local x ="), ["=: Expression expected"]);
        assert_eq!(problems("if a then\n  f()\n"), ["): 'end' expected"]);
        assert_eq!(problems("x = 1 )\nlocal ok = 1"), ["): Unexpected symbol ')'"]);
        assert_eq!(problems("t = { 1 2 }"), ["1: , or ; expected"]);
        assert_eq!(problems("f(a, )"), [",: Expression expected"]);
        assert!(problems("local t <const> = { .a, [k] = `h` }\nt.x += 1\nlocal a, b in t\n/* c */ f(t?.x)").is_empty());
    }

    #[test]
    fn statements_that_are_not_statements() {
        assert_eq!(problems("foo"), ["foo: syntax error"]);
        assert_eq!(problems("a.b.c\nf()"), ["a.b.c: syntax error"]);
        assert_eq!(problems("x = a:b"), ["a:b: Arguments expected"]);
        assert!(problems("a:b()\n(f)()\nf{}\nf'x'").is_empty());
    }

    #[test]
    fn a_named_operator_is_reported_once() {
        assert_eq!(problems("i++").len(), 1);
        assert_eq!(problems("if a != b then f() end").len(), 1);
        assert_eq!(problems("x //= 2").len(), 1);
        assert_eq!(problems("if a && b then f() end").len(), 1);
    }

    #[test]
    fn compound_assignment_to_const_and_close() {
        assert_eq!(
            problems(
                "local c <const> = 1\nc += 1\nlocal f <close> = nil\nf -= 1\nlocal v = 1\nv += 1\nlocal t = {}\nt.c = 0\nt.c += 1"
            ),
            [
                format!("c: Compound assignment to <const> variable 'c': {CONST}"),
                format!("f: Compound assignment to <close> variable 'f': {CONST}"),
            ]
        );
        assert_eq!(
            problems("local src = { k = 1 }\nlocal k <const> in src\nk *= 2"),
            [format!("k: Compound assignment to <const> variable 'k': {CONST}")]
        );
        // A shadowing local and a parameter are other variables.
        assert!(problems("local c <const> = 1\ndo local c = 2 c += 1 end\nlocal function f(c) c += 1 end").is_empty());
    }

    #[test]
    fn close_in_unpacking() {
        assert_eq!(
            problems("local src = { a = 1, b = 2 }\nlocal a <close>, b <const> in src\nlocal f <close> = nil"),
            ["<close>: The <close> attribute is ignored in an 'in' unpacking: the value is never closed"]
        );
    }

    #[test]
    fn xor_assignment() {
        assert_eq!(problems("local x = 3\nx ^= 2\nx |= 1"), ["^=: In CfxLua '^=' is bitwise XOR, not exponentiation"]);
        let src = Source::new("x ^= 2");
        assert_eq!(check(&src)[0].severity, Severity::Info);
        assert_eq!(
            fixed("local x = 3\nx ^= 1 + 1", "Replace with 'x = x ^ (rhs)' (exponentiation)"),
            "local x = 3\nx = x ^ (1 + 1)"
        );
        assert!(check(&Source::new("x ^=")).iter().all(|p| p.fixes.is_empty()), "no value, no fix");
    }

    #[test]
    fn decrement_starts_a_comment() {
        let warn = "--: CfxLua has no '--' decrement operator: this starts a comment";
        assert_eq!(
            problems(
                "local i, t = 1, { n = 1 }\nlocal x = i-- garbage\nt.n--\nt[1]-- note\nlocal y = i -- a comment\n\
                 print(i)--comment after a call\nlocal z = 1--comment after a number\nlocal w--\ni---doc\ni--region x\na--=1\na:m--"
            ),
            // Before a documentation or region comment the name is only a statement that is not one.
            [warn, warn, warn, "i: syntax error", "i: syntax error", warn, warn]
        );
        assert_eq!(
            fixed("local i = 1\nlocal t = { n = 1 }\nt.n-- count down", "Replace with 't.n -= 1'"),
            "local i = 1\nlocal t = { n = 1 }\nt.n -= 1 -- count down"
        );
        assert_eq!(fixed("local i = 1\ni--", "Replace with 'i -= 1'"), "local i = 1\ni -= 1");
        let src = Source::new("local x = i-- garbage");
        assert!(check(&src)[0].fixes.is_empty(), "not a whole statement");
        assert_eq!(check(&src)[0].severity, Severity::Warning);
    }

    #[test]
    fn increment_parse_error() {
        assert_eq!(problems("i++\nlocal y = x++"), ["++: '++' is not supported in CfxLua, use 'x += 1'"; 2]);
        assert_eq!(fixed("local i = 0\ni++", "Replace with 'i += 1'"), "local i = 0\ni += 1");
        assert_eq!(fixed("local t = { n = 0 }\n++t.n", "Replace with 't.n += 1'"), "local t = { n = 0 }\nt.n += 1");
        let src = Source::new("local y = x++\nf()++\n++\ng()");
        assert!(check(&src).iter().all(|p| p.fixes.is_empty()));
    }

    #[test]
    fn operators_of_other_languages() {
        assert_eq!(
            problems("if a != b and !c then end\nif a&&b || c then end"),
            [
                "!=: '!=' is not supported in CfxLua, use '~='",
                "!: '!' is not supported in CfxLua, use 'not'",
                "&&: '&&' is not supported in CfxLua, use 'and'",
                "||: '||' is not supported in CfxLua, use 'or'",
            ]
        );
        assert_eq!(fixed("if a != b then end", "Replace with '~='"), "if a ~= b then end");
        assert_eq!(fixed("if !c then end", "Replace with 'not'"), "if not c then end");
        assert_eq!(fixed("if a&&b then end", "Replace with 'and'"), "if a and b then end");
        assert_eq!(fixed("x = a || b", "Replace with 'or'"), "x = a or b");
        // `a & b`, `a | ~b` and `a ~= b` are Lua 5.4.
        assert!(problems("x = a & b | ~c\nif a ~= b then end").is_empty());
    }

    #[test]
    fn compound_operators_cfxlua_lacks() {
        assert_eq!(
            problems("n //= 2\nn %= 3\ns ..= 'x'"),
            [
                "//=: '//=' is not supported in CfxLua, use 'x = x // y'",
                "%=: '%=' is not supported in CfxLua, use 'x = x % y'",
                "..=: '..=' is not supported in CfxLua, use 'x = x .. y'",
            ]
        );
        assert_eq!(fixed("n //= 2", "Replace with 'n = n // 2'"), "n = n // 2");
        assert_eq!(fixed("t.n %= a + 1", "Replace with 't.n = t.n % (a + 1)'"), "t.n = t.n % (a + 1)");
        assert_eq!(fixed("s ..= f(x)", "Replace with 's = s .. f(x)'"), "s = s .. f(x)");
        // The nine CfxLua operators are fine.
        assert!(problems("a += 1 a -= 1 a *= 1 a /= 1 a <<= 1 a >>= 1 a &= 1 a |= 1").is_empty());
    }

    #[test]
    fn lexer_facts_the_checks_rely_on() {
        let k = |s: &str| tokenize(s).iter().map(|t| (t.kind, t.text(s).to_owned())).collect::<Vec<_>>();
        let op = |s: &str| (Tok::Op, s.to_owned());
        let name = |s: &str| (Tok::Name, s.to_owned());
        assert_eq!(k("a-=1"), [name("a"), op("-="), (Tok::Number, "1".into())]);
        assert_eq!(k("a--=1"), [name("a"), (Tok::LineComment, "--=1".into())]);
        assert_eq!(k("i--"), [name("i"), (Tok::LineComment, "--".into())]);
        for o in ["+=", "-=", "*=", "/=", "<<=", ">>=", "&=", "|=", "^=", "<=", "<<", ">=", ">>", "~=", "=="] {
            assert_eq!(k(&format!("a{o}b"))[1], op(o));
        }
        assert_eq!(k("a//2")[1], op("//"));
        assert_eq!(k("return 1 // comment")[2..], [op("//"), name("comment")]);
        assert_eq!(k("/* a /* b */ c */"), [(Tok::CComment, "/* a /* b */".into()), name("c"), op("*"), op("/")]);
        assert_eq!(k("/*/ x */"), [(Tok::CComment, "/*/ x */".into())]);
        assert_eq!(k("--/* x\ny")[1], name("y"));
        assert_eq!(k("{ .a, .5, ... }")[1..6], [op("."), name("a"), op(","), (Tok::Number, ".5".into()), op(",")]);
        assert_eq!(k("local x <const> = 1")[2..5], [op("<"), name("const"), op(">")]);
        assert_eq!(k("f`x`"), [name("f"), (Tok::Hash { closed: true }, "`x`".into())]);
        assert_eq!(k("`abc\nx")[0], (Tok::Hash { closed: false }, "`abc".into()));
        assert_eq!(k("a**b")[1..3], [op("*"), op("*")]);
    }

    #[test]
    fn feature_reads_lua_documents_only() {
        let fx = Fixture::new(&[("res/client.lua", "i++"), ("res/web/app.js", "i++")]);
        let feature = SyntaxFeature;
        assert_eq!(feature.problems(&fx.cx(&fx.doc("res/client.lua"))).len(), 1);
        assert!(feature.problems(&fx.cx(&fx.doc("res/web/app.js"))).is_empty());
        let fx = fx.with_settings(crate::settings::Settings::defaults().with("enabled", false.into()));
        assert!(feature.problems(&fx.cx(&fx.doc("res/client.lua"))).is_empty());
    }
}
