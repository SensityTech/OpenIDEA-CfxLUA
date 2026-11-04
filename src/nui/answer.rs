use super::lua::{Analysis, CALLBACK_FUNCTIONS, string_value};
use super::web::NUI_EVENT_PREFIX;
use crate::feature::{Fix, Problem};
use crate::syntax::tree::{Block, Kind, Span, Stmt};
use openidea_plugin::Severity;

const CODE: &str = "CfxNuiCallbackWithoutCb";
const RESPONDER_NAMES: &[&str] = &["cb", "respond", "callback"];

pub fn check(an: &Analysis) -> Vec<Problem> {
    let src = an.src;
    let mut out = Vec::new();
    for (i, call) in an.calls.iter().enumerate() {
        let Some(callee) = an.plain_callee(call) else { continue };
        let mut args = an.args(i).iter().copied();
        let Some(name_arg) = args.next() else { continue };
        let literal = (name_arg.1 == name_arg.0 + 1).then(|| string_value(src, name_arg.0)).flatten();
        let nui_event =
            callee == "AddEventHandler" && literal.as_ref().is_some_and(|(v, _)| v.starts_with(NUI_EVENT_PREFIX));
        if !CALLBACK_FUNCTIONS.contains(&callee) && !nui_event {
            continue;
        }
        let Some(f) = args.next().and_then(|s| an.closure(s)) else { continue };
        let func = &an.tree.funcs[f];
        if func.vararg {
            continue;
        }
        let cb = func.params.get(1).copied();
        let cb_decl = cb.and_then(|t| an.scopes.decl_of(t));
        let name = match &literal {
            Some((v, _)) => v.strip_prefix(NUI_EVENT_PREFIX).unwrap_or(v).to_owned(),
            None => text(src, name_arg).to_owned(),
        };
        let mut flow = Flow { an, func: f, cb: cb_decl, failures: Vec::new() };
        if flow.escapes() {
            continue;
        }
        let result = flow.statements(&func.body.stmts, false);
        if result != Some(false) && flow.failures.is_empty() {
            continue;
        }
        let cb_name = cb.map_or_else(|| free_name(an, f), |t| src.tok_text(t).to_owned());
        let answer = if callee.starts_with("RegisterRaw") {
            format!("{cb_name}({{ status = 200, body = '' }})")
        } else {
            format!("{cb_name}('ok')")
        };
        let mut fix = Fix::new(format!("Call {answer} on every path"));
        if cb.is_none() {
            let Some(close) = func.close else { continue };
            // `_`: a new `data` parameter would hide an upvalue of that name.
            let param = if func.params.is_empty() { format!("_, {cb_name}") } else { format!(", {cb_name}") };
            fix = fix.insert(src.toks[close].start, param);
        }
        for &r in &flow.failures {
            let (at, text) = insert_before(src, src.toks[r].start, &answer);
            fix = fix.insert(at, text);
        }
        if result == Some(false) {
            let (at, text) = insert_last(an, f, &func.body, &answer);
            fix = fix.insert(at, text);
        }
        if result == Some(false) {
            let message = if cb.is_none() {
                format!(
                    "NUI callback '{name}' never answers: its function takes no cb parameter, so the page's request waits until it times out"
                )
            } else {
                format!(
                    "NUI callback '{name}' can end without calling {cb_name}(...): the page's request waits until it times out"
                )
            };
            let (s, e) = src.span(name_arg.0, name_arg.1);
            out.push(Problem::new(CODE, Severity::Info, s, e, message).fix(fix.clone()));
        }
        for &r in &flow.failures {
            let t = src.toks[r];
            let message = format!(
                "Returns without calling {cb_name}(...): the page's request for '{name}' waits until it times out"
            );
            out.push(Problem::new(CODE, Severity::Info, t.start, t.end, message).fix(fix.clone()));
        }
    }
    out
}

fn text(src: &crate::lua::Source, (a, b): Span) -> &str {
    let (s, e) = src.span(a, b);
    &src.text[s..e]
}

struct Flow<'a> {
    an: &'a Analysis<'a>,
    func: usize,
    cb: Option<usize>,
    failures: Vec<usize>,
}

impl Flow<'_> {
    fn escapes(&self) -> bool {
        let Some(cb) = self.cb else { return false };
        let f = &self.an.tree.funcs[self.func];
        self.an.scopes.uses(cb, f.kw, f.next).any(|t| !self.direct_call(t))
    }

    fn direct_call(&self, t: usize) -> bool {
        let src = self.an.src;
        let args = src.is_op(t + 1, "(") || src.is_op(t + 1, "{") || src.toks.get(t + 1).is_some_and(|n| n.is_string());
        args && self.an.tree.owner(t) == Some(self.func)
    }

    fn calls_cb(&self, (a, b): Span) -> bool {
        let Some(cb) = self.cb else { return false };
        self.an.scopes.uses(cb, a, b).any(|t| self.direct_call(t))
    }

    fn statements(&mut self, stmts: &[Stmt], called_before: bool) -> Option<bool> {
        let mut called = called_before;
        for s in stmts {
            if called {
                return Some(true);
            }
            match &s.kind {
                Kind::Return { .. } => {
                    if !self.calls_cb((s.start, s.end)) {
                        self.failures.push(s.start);
                    }
                    return None;
                }
                Kind::If { conds, blocks, has_else } => {
                    let mut results: Vec<Option<bool>> =
                        blocks.iter().map(|b| self.statements(&b.stmts, false)).collect();
                    if !has_else {
                        results.push(Some(false));
                    }
                    let reaching: Vec<bool> = results.into_iter().flatten().collect();
                    if reaching.is_empty() {
                        return None;
                    }
                    called = reaching.iter().all(|&r| r) || conds.iter().any(|c| self.calls_cb(*c));
                }
                Kind::Do(body) => called = self.statements(&body.stmts, false)?,
                Kind::Repeat { body, .. } => {
                    // A `break` may leave the body early: only a call reached on every path counts.
                    called = self.statements(&body.stmts, false) == Some(true) && !has_break(body);
                }
                Kind::While { cond, body } => {
                    // The body may not run: only its returns matter; the condition runs at least once.
                    self.statements(&body.stmts, false);
                    called = self.calls_cb(*cond);
                }
                Kind::For { body, .. } => {
                    self.statements(&body.stmts, false);
                    called = false;
                }
                Kind::Expr(span) if self.is_error(*span) => return None,
                _ => {
                    if self.calls_cb((s.start, s.end)) {
                        called = true;
                    }
                }
            }
        }
        Some(called)
    }

    fn is_error(&self, (a, b): Span) -> bool {
        let src = self.an.src;
        src.tok_text(a) == "error"
            && src.is_name(a)
            && ((src.is_op(a + 1, "(") && src.pair[a + 1] == Some(b - 1))
                || (b == a + 2 && src.toks[a + 1].is_string()))
    }
}

fn has_break(b: &Block) -> bool {
    b.stmts.iter().any(|s| match &s.kind {
        Kind::Break => true,
        Kind::If { blocks, .. } => blocks.iter().any(has_break),
        Kind::Do(inner) => has_break(inner),
        _ => false,
    })
}

fn free_name(an: &Analysis, f: usize) -> String {
    let src = an.src;
    let func = &an.tree.funcs[f];
    let used: Vec<&str> =
        (func.kw..func.next.min(src.toks.len())).filter(|&i| src.is_name(i)).map(|i| src.tok_text(i)).collect();
    if let Some(n) = RESPONDER_NAMES.iter().find(|n| !used.contains(n)) {
        return (*n).to_owned();
    }
    (2..).map(|n| format!("cb{n}")).find(|n| !used.contains(&n.as_str())).unwrap_or_default()
}

fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |i| i + 1)
}

fn line_end(text: &str, at: usize) -> usize {
    text[at..].find('\n').map_or(text.len(), |i| at + i)
}

fn indent_of(text: &str, at: usize) -> &str {
    let ls = line_start(text, at);
    let n = text[ls..].bytes().take_while(|b| *b == b' ' || *b == b'\t').count();
    &text[ls..ls + n]
}

fn starts_line(text: &str, at: usize) -> bool {
    text[line_start(text, at)..at].trim().is_empty()
}

fn insert_before(src: &crate::lua::Source, at: usize, line: &str) -> (usize, String) {
    let text = &src.text;
    if starts_line(text, at) {
        (line_start(text, at), format!("{}{line}\n", indent_of(text, at)))
    } else {
        (at, format!("{line} "))
    }
}

fn insert_last(an: &Analysis, f: usize, body: &Block, line: &str) -> (usize, String) {
    let src = an.src;
    let text = &src.text;
    let func = &an.tree.funcs[f];
    let Some(last) = body.stmts.last() else {
        let start = func.close.or(Some(func.kw)).map_or(0, |t| src.toks[t].end);
        let end = func.end.map_or(text.len(), |e| src.toks[e].end);
        let kw = src.toks[func.kw].start;
        return if text[kw..end].contains('\n') {
            (start, format!("\n{}{}{line}", indent_of(text, kw), indent_unit(text)))
        } else {
            (start, format!(" {line}"))
        };
    };
    let (start, end) = src.span(last.start, last.end);
    if !starts_line(text, start) {
        return (end, format!(" {line}"));
    }
    // After a comment ending the line of the statement.
    let next = src.all.partition_point(|t| t.start < end);
    let comment = src.all.get(next).filter(|t| t.is_comment() && !text[end..t.start].contains('\n'));
    let at = comment.map_or(end, |c| c.end);
    let at = if text[at..line_end(text, at)].trim().is_empty() { at } else { end };
    (at, format!("\n{}{line}", indent_of(text, start)))
}

fn indent_unit(text: &str) -> &str {
    text.split('\n')
        .find_map(|l| {
            let n = l.bytes().take_while(|b| *b == b' ' || *b == b'\t').count();
            (n > 0 && n < l.trim_end().len()).then(|| &l[..n])
        })
        .unwrap_or("    ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::Source;

    fn problems(text: &str) -> Vec<String> {
        let src = Source::new(text);
        check(&Analysis::new(&src)).iter().map(|p| format!("{}: {}", &text[p.start..p.end], p.message)).collect()
    }

    fn fixed(text: &str, title: &str) -> String {
        let src = Source::new(text);
        let problems = check(&Analysis::new(&src));
        let fix = problems.iter().flat_map(|p| &p.fixes).find(|f| f.title == title).unwrap_or_else(|| {
            panic!("no '{title}' in {:?}", problems.iter().flat_map(|p| &p.fixes).map(|f| &f.title).collect::<Vec<_>>())
        });
        let mut out = text.to_owned();
        let mut edits = fix.edits.clone();
        edits.sort_by_key(|e| std::cmp::Reverse(e.0));
        for (s, e, t) in edits {
            out.replace_range(s..e, &t);
        }
        out
    }

    const WAIT: &str = "the page's request";

    #[test]
    fn an_early_return_and_an_end_without_cb() {
        assert_eq!(
            problems(
                "RegisterNUICallback('buy', function(data, cb)\n    if not data.item then\n        return\n    end\n\
                     TriggerServerEvent('shop:buy', data.item)\nend)"
            ),
            [
                format!("'buy': NUI callback 'buy' can end without calling cb(...): {WAIT} waits until it times out"),
                format!("return: Returns without calling cb(...): {WAIT} for 'buy' waits until it times out"),
            ]
        );
    }

    #[test]
    fn branches_and_loops_that_may_not_answer() {
        assert_eq!(
            problems(
                "RegisterNUICallback('toggle', function(data, cb)\n    if data.on then\n        cb(true)\n    end\nend)\n\
                 RegisterNUICallback('each', function(data, respond)\n    for _, item in ipairs(data.items) do\n        respond(item)\n    end\nend)\n\
                 AddEventHandler('__cfx_nui:close', function(data, cb)\nend)"
            ),
            [
                format!(
                    "'toggle': NUI callback 'toggle' can end without calling cb(...): {WAIT} waits until it times out"
                ),
                format!(
                    "'each': NUI callback 'each' can end without calling respond(...): {WAIT} waits until it times out"
                ),
                format!(
                    "'__cfx_nui:close': NUI callback 'close' can end without calling cb(...): {WAIT} waits until it times out"
                ),
            ]
        );
    }

    #[test]
    fn commas_in_the_function_are_not_arguments() {
        let src =
            Source::new("RegisterNUICallback('x', function(data, cb)\n    local a, b = 1, 2\n    return a, b\nend)");
        let an = Analysis::new(&src);
        assert_eq!(an.args(0).len(), 2);
        assert_eq!(check(&an).len(), 1);
    }

    #[test]
    fn a_callback_function_without_a_cb_parameter() {
        assert_eq!(
            problems("RegisterNUICallback('close', function()\n    SetNuiFocus(false, false)\nend)"),
            [format!(
                "'close': NUI callback 'close' never answers: its function takes no cb parameter, so {WAIT} waits until it times out"
            )]
        );
    }

    #[test]
    fn callbacks_answering_on_every_path_are_not_reported() {
        assert!(
            problems(
                "RegisterNUICallback('guarded', function(data, cb)\n    if not data.item then\n        cb(false)\n        return\n    end\n\
                     if data.a then cb(1) elseif data.b then cb(2) else cb(3) end\nend)\n\
                 RegisterNUICallback('returned', function(data, cb)\n    if data.x then return cb('x') end\n    cb('ok')\nend)\n\
                 RegisterNUICallback('errors', function(data, cb)\n    if not data then error('no data') end\n    cb('ok')\nend)\n\
                 RegisterNUICallback('block', function(data, cb)\n    do\n        cb('ok')\n    end\nend)\n\
                 RegisterNUICallback('repeat', function(data, cb)\n    repeat\n        cb('ok')\n    until true\nend)\n\
                 RegisterRawNUICallback('raw', function(req, cb)\n    cb({ body = '{}', headers = { ['Content-Type'] = 'application/json' } })\nend)"
            )
            .is_empty()
        );
    }

    #[test]
    fn callbacks_answered_asynchronously_or_passed_on_stay_silent() {
        assert!(
            problems(
                "local pending = {}\nRegisterNUICallback('async', function(data, cb)\n    lib.callback('shop:get', false, function(result)\n        cb(result)\n    end)\nend)\n\
                 RegisterNUICallback('passed', function(data, cb)\n    ESX.TriggerServerCallback('shop:get', cb, data)\nend)\n\
                 RegisterNUICallback('stored', function(data, cb)\n    pending[#pending + 1] = cb\nend)\n\
                 RegisterNUICallback('thread', function(data, cb)\n    CreateThread(function()\n        Wait(100)\n        cb('ok')\n    end)\nend)\n\
                 RegisterNUICallback('vararg', function(...)\nend)\nRegisterNUICallback('reference', OnReference)\n\
                 RegisterNUICallback(dynamicName, function(data, cb)\n    cb('ok')\nend)"
            )
            .is_empty()
        );
    }

    #[test]
    fn the_quick_fix_answers_before_each_return_and_at_the_end() {
        let text = "RegisterNUICallback('buy', function(data, cb)\n    if not data.item then return end\n    TriggerServerEvent('shop:buy', data.item)\nend)\n\
                    RegisterNUICallback('other', function(data, cb)\n    cb('ok')\nend)";
        assert_eq!(
            fixed(text, "Call cb('ok') on every path"),
            "RegisterNUICallback('buy', function(data, cb)\n    if not data.item then cb('ok') return end\n    TriggerServerEvent('shop:buy', data.item)\n    cb('ok')\nend)\n\
             RegisterNUICallback('other', function(data, cb)\n    cb('ok')\nend)"
        );
        let after = fixed(text, "Call cb('ok') on every path");
        assert!(problems(&after).is_empty());
    }

    #[test]
    fn the_quick_fix_adds_the_cb_parameter() {
        assert_eq!(
            fixed(
                "RegisterNUICallback('close', function()\n    SetNuiFocus(false, false)\nend)",
                "Call cb('ok') on every path"
            ),
            "RegisterNUICallback('close', function(_, cb)\n    SetNuiFocus(false, false)\n    cb('ok')\nend)"
        );
        assert_eq!(
            fixed(
                "RegisterNUICallback('x', function(data, cb)\n    if data then\n        return\n    end\nend)",
                "Call cb('ok') on every path"
            ),
            "RegisterNUICallback('x', function(data, cb)\n    if data then\n        cb('ok')\n        return\n    end\n    cb('ok')\nend)"
        );
        assert_eq!(
            fixed("AddEventHandler('__cfx_nui:x', function(data, cb) end)", "Call cb('ok') on every path"),
            "AddEventHandler('__cfx_nui:x', function(data, cb) cb('ok') end)"
        );
    }

    #[test]
    fn the_added_parameters_hide_no_upvalue() {
        assert_eq!(
            fixed(
                "local data, cb = { items = {} }, nil\nRegisterNUICallback('items', function()\n    SendNUIMessage({ items = data.items, cb = cb })\nend)",
                "Call respond('ok') on every path"
            ),
            "local data, cb = { items = {} }, nil\nRegisterNUICallback('items', function(_, respond)\n    SendNUIMessage({ items = data.items, cb = cb })\n    respond('ok')\nend)"
        );
    }

    #[test]
    fn a_raw_callback_is_answered_with_a_response_table() {
        let text = "RegisterRawNUICallback('raw', function(request, respond)\n    if not request.body then return end\n    respond({ status = 200, body = '{}' })\nend)";
        assert_eq!(
            problems(text),
            [format!("return: Returns without calling respond(...): {WAIT} for 'raw' waits until it times out")]
        );
        assert_eq!(
            fixed(text, "Call respond({ status = 200, body = '' }) on every path"),
            "RegisterRawNUICallback('raw', function(request, respond)\n    if not request.body then respond({ status = 200, body = '' }) return end\n    respond({ status = 200, body = '{}' })\nend)"
        );
    }
}
