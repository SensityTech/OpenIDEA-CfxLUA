use super::ast::{Ast, ExprKind, Node, StatKind};
use crate::lua::Source;

pub type Edit = (usize, usize, String);

pub fn normalize(mut edits: Vec<Edit>) -> Vec<Edit> {
    let mut indexed: Vec<(usize, Edit)> = edits.drain(..).enumerate().collect();
    indexed.sort_by_key(|(i, e)| (e.0, e.1 > e.0, *i));
    let mut out: Vec<Edit> = Vec::new();
    for (_, e) in indexed {
        match out.last_mut() {
            Some(last) if last.0 == e.0 && last.1 == last.0 => {
                last.1 = e.1;
                last.2.push_str(&e.2);
            }
            _ => out.push(e),
        }
    }
    out
}

pub fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |i| i + 1)
}

pub fn line_end(text: &str, at: usize) -> usize {
    text[at..].find('\n').map_or(text.len(), |i| at + i)
}

fn blank(s: &str) -> bool {
    s.chars().all(char::is_whitespace)
}

pub fn starts_line(text: &str, at: usize) -> bool {
    blank(&text[line_start(text, at)..at])
}

pub fn indent_of(text: &str, at: usize) -> &str {
    let ls = line_start(text, at);
    let rest = &text[ls..];
    &rest[..rest.len() - rest.trim_start_matches([' ', '\t']).len()]
}

fn body_of(ast: &Ast, n: Node) -> Option<usize> {
    match n {
        Node::Block(b) => Some(b),
        Node::Stat(s) => match &ast.stats[s].kind {
            StatKind::Do(b) | StatKind::While { body: b, .. } | StatKind::Repeat { body: b, .. } => Some(*b),
            StatKind::NumFor { body, .. } | StatKind::GenFor { body, .. } => Some(*body),
            StatKind::If { blocks, .. } => blocks.first().copied(),
            _ => None,
        },
        Node::Expr(e) => match ast.exprs[e].kind {
            ExprKind::Function { body, .. } => Some(body),
            _ => None,
        },
    }
}

pub fn indent_unit(ast: &Ast, src: &Source, n: Node) -> String {
    let text = &src.text;
    let mut x = Some(n);
    while let Some(node) = x {
        if let Some(body) = body_of(ast, node)
            && let Some(owner) = ast.blocks[body].parent
        {
            let first = ast.blocks[body].stats.iter().find(|&&s| starts_line(text, ast.stats[s].start));
            let owner_start = ast.span(owner).0;
            if let Some(&first) = first
                && starts_line(text, owner_start)
            {
                let inner = indent_of(text, ast.stats[first].start);
                let outer = indent_of(text, owner_start);
                if inner.len() > outer.len() && inner.starts_with(outer) {
                    return inner[outer.len()..].to_owned();
                }
            }
        }
        x = ast.parent(node);
    }
    "    ".into()
}

pub fn trailing_comment(src: &Source, end: usize) -> Option<(usize, usize)> {
    let i = src.all.partition_point(|t| t.start < end);
    let t = src.all.get(i)?;
    let gap = &src.text[end..t.start];
    (t.is_comment() && blank(gap) && !gap.contains('\n')).then_some((t.start, t.end))
}

pub fn end_of_line(src: &Source, end: usize) -> usize {
    let text = &src.text;
    let with_comment = trailing_comment(src, end).map_or(end, |c| c.1);
    if blank(&text[with_comment..line_end(text, with_comment)]) { with_comment } else { end }
}

pub fn delete(src: &Source, (start, end): (usize, usize)) -> Edit {
    let text = &src.text;
    let stop = end_of_line(src, end);
    let ls = line_start(text, start);
    let le = line_end(text, stop);
    if blank(&text[ls..start]) && blank(&text[stop..le]) {
        return if le < text.len() { (ls, le + 1, String::new()) } else { (ls.saturating_sub(1), le, String::new()) };
    }
    (start, end, String::new())
}

pub fn insert_before(src: &Source, start: usize, line: &str) -> Edit {
    let text = &src.text;
    if starts_line(text, start) {
        let ls = line_start(text, start);
        (ls, ls, format!("{}{line}\n", indent_of(text, start)))
    } else {
        (start, start, format!("{line} "))
    }
}

pub fn insert_first(ast: &Ast, src: &Source, owner: Node, block: usize, line: &str) -> Edit {
    let text = &src.text;
    let Some(&first) = ast.blocks[block].stats.first() else { return insert_into_empty(ast, src, owner, block, line) };
    let start = ast.stats[first].start;
    if !starts_line(text, start) {
        return insert_before(src, start, line);
    }
    let mut top = start;
    let mut i = src.all.partition_point(|t| t.start < start);
    while i > 0 {
        let t = src.all[i - 1];
        if text[t.end..top].matches('\n').count() > 1 || !t.is_comment() || !starts_line(text, t.start) {
            break;
        }
        top = t.start;
        i -= 1;
    }
    let at = line_start(text, top);
    (at, at, format!("{}{line}\n", indent_of(text, start)))
}

pub fn insert_last(ast: &Ast, src: &Source, owner: Node, block: usize, line: &str, exclude: Option<usize>) -> Edit {
    let text = &src.text;
    let Some(&last) = ast.blocks[block].stats.iter().rev().find(|&&s| Some(s) != exclude) else {
        return insert_into_empty(ast, src, owner, block, line);
    };
    let s = &ast.stats[last];
    if starts_line(text, s.start) {
        let at = end_of_line(src, s.stop);
        (at, at, format!("\n{}{line}", indent_of(text, s.start)))
    } else {
        (s.stop, s.stop, format!(" {line}"))
    }
}

fn insert_into_empty(ast: &Ast, src: &Source, owner: Node, block: usize, line: &str) -> Edit {
    let text = &src.text;
    let at = ast.blocks[block].start;
    let (os, oe) = ast.span(owner);
    if text[os..oe].contains('\n') {
        (at, at, format!("\n{}{}{line}", indent_of(text, os), indent_unit(ast, src, owner)))
    } else {
        (at, at, format!(" {line}"))
    }
}

pub fn fresh_name(ast: &Ast, scope: usize, base: &str) -> String {
    let mut used = std::collections::HashSet::new();
    ast.walk_all(Node::Stat(scope), &mut |n| {
        if let Node::Expr(e) = n
            && let Some(name) = ast.name_of(e)
        {
            used.insert(name.to_owned());
        }
    });
    let (first, end) = (ast.stats[scope].first, ast.stats[scope].end);
    used.extend(ast.decls.iter().filter(|d| (first..end).contains(&d.tok)).map(|d| d.name.clone()));
    (1..)
        .map(|i| if i == 1 { base.to_owned() } else { format!("{base}{i}") })
        .find(|n| !used.contains(n) && ast.local_at(n, first).is_none())
        .unwrap_or_else(|| base.to_owned())
}

pub fn wait_function(text: &str) -> &'static str {
    let citizen = find_call(
        text,
        |t, i| {
            let rest = t[i + "Citizen".len()..].trim_start();
            rest.strip_prefix('.')
                .is_some_and(|r| r.trim_start().strip_prefix("Wait").is_some_and(|r| r.trim_start().starts_with('(')))
        },
        "Citizen",
    );
    let plain = find_call(
        text,
        |t, i| {
            let before = t[..i].chars().next_back();
            before.is_none_or(|c| !(c == '.' || c.is_alphanumeric() || c == '_'))
                && t[i + 4..].trim_start().starts_with('(')
        },
        "Wait",
    );
    if citizen && !plain { "Citizen.Wait" } else { "Wait" }
}

fn find_call(text: &str, ok: impl Fn(&str, usize) -> bool, word: &str) -> bool {
    text.match_indices(word).any(|(i, _)| ok(text, i))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_and_merges() {
        assert_eq!(wait_function("Citizen.Wait(0)"), "Citizen.Wait");
        assert_eq!(wait_function("Citizen.Wait(0) Wait (1)"), "Wait");
        assert_eq!(wait_function("x.Wait(1)"), "Wait");
        assert_eq!(
            normalize(vec![(5, 5, "b".into()), (1, 2, "x".into()), (5, 5, "c".into()), (5, 7, "r".into())]),
            [(1, 2, "x".to_owned()), (5, 7, "bcr".to_owned())]
        );
    }

    #[test]
    fn lines() {
        let src = Source::new("while true do\n    print(1) -- c\n    x()\nend");
        let ast = super::super::ast::parse(&src);
        let w = ast.blocks[ast.root].stats[0];
        let StatKind::While { body, .. } = ast.stats[w].kind else { panic!() };
        let print = ast.blocks[body].stats[0];
        let (start, stop) = (ast.stats[print].start, ast.stats[print].stop);
        assert_eq!(delete(&src, (start, stop)), (14, 32, String::new()));
        assert_eq!(indent_unit(&ast, &src, Node::Stat(print)), "    ");
        assert_eq!(insert_first(&ast, &src, Node::Stat(w), body, "local a = 1"), (14, 14, "    local a = 1\n".into()));
        let last = insert_last(&ast, &src, Node::Stat(w), body, "Wait(0)", None);
        assert_eq!(last.2, "\n    Wait(0)");
        assert_eq!(fresh_name(&ast, w, "x"), "x2");
        assert_eq!(fresh_name(&ast, w, "sleep"), "sleep");
    }
}
