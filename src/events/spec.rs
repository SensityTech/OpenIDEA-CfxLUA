use super::model::{Dirs, EventFn, FnKind, KeySpec, Origin, Role, Seg, is_standard_name};
use crate::project::Side;
use std::rc::Rc;

pub const ERROR_KIND: &str = "Unknown event kind: start with handler, trigger, register or alias";
pub const ERROR_FUNCTION: &str = "---@cfx.event must document a function";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec {
    pub role: Role,
    pub key: Vec<usize>,
    pub logical: bool,
    pub callback: Option<usize>,
    pub injected: usize,
    pub net: bool,
    pub directions: Option<Dirs>,
    pub side: Option<Side>,
    pub target: Option<usize>,
    pub payload: Option<usize>,
    pub wire: Option<usize>,
    pub errors: Vec<String>,
}

enum Token<'a> {
    Word(&'a str, Option<&'a str>),
    Char(char),
}

fn tokens(text: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let mut rest = text;
    loop {
        rest = rest.trim_start();
        let Some(c) = rest.chars().next() else { break };
        if c.is_ascii_alphabetic() || c == '_' {
            let len = rest.find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_')).unwrap_or(rest.len());
            let word = &rest[..len];
            let after = rest[len..].trim_start();
            if let Some(inner) = after.strip_prefix('(')
                && let Some(close) = inner.find(')')
            {
                out.push(Token::Word(word, Some(&inner[..close])));
                rest = &inner[close + 1..];
            } else {
                out.push(Token::Word(word, None));
                rest = &rest[len..];
            }
        } else {
            out.push(Token::Char(c));
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

impl Spec {
    pub fn parse(text: &str, params: Option<&[String]>, vararg: bool) -> Option<Spec> {
        let names: &[String] = params.unwrap_or(&[]);
        let mut errors = Vec::new();
        let mut role = None;
        let mut key: Option<Vec<usize>> = None;
        let mut logical = false;
        let mut callback = None;
        let mut injected = 0;
        let mut net = true;
        let mut directions: Option<Dirs> = None;
        let mut side = None;
        let mut target = None;
        let mut payload = None;
        let mut wire = None;

        let reference = |value: &str, allow_vararg: bool, errors: &mut Vec<String>| -> Option<usize> {
            let v = value.trim();
            if v == "..." {
                return match params {
                    _ if !allow_vararg => {
                        errors.push("'...' is only valid in payload()".into());
                        None
                    }
                    None => {
                        errors.push("'...' needs the function's parameters: use a position".into());
                        None
                    }
                    Some(p) => Some(p.len()),
                };
            }
            if let Ok(n) = v.parse::<i64>() {
                if n >= 1 {
                    return Some(n as usize - 1);
                }
                errors.push("Parameter positions start at 1".into());
                return None;
            }
            let found = names.iter().position(|n| n == v);
            if found.is_none() {
                errors.push(format!("Unknown parameter '{v}'"));
            }
            found
        };

        for token in tokens(text) {
            let (word, args) = match token {
                Token::Char(',' | ';') => continue,
                // the rest is a description
                Token::Char(_) => break,
                Token::Word(w, a) => (w, a),
            };
            let lower = word.to_ascii_lowercase();
            let Some(r) = role else {
                role = Some(match lower.as_str() {
                    "handler" => Role::Handler,
                    "trigger" => Role::Trigger,
                    "register" => Role::Register,
                    "alias" => Role::Alias,
                    _ => return None,
                });
                continue;
            };
            let args_text = args.unwrap_or("");
            match lower.as_str() {
                "net" => net = true,
                "local" => {
                    net = false;
                    directions = Some(directions.unwrap_or_default().with(Dirs::LOCAL));
                }
                "server" | "client" | "both" => {
                    if r == Role::Trigger {
                        let mut d = directions.unwrap_or_default();
                        if lower != "client" {
                            d = d.with(Dirs::SERVER);
                        }
                        if lower != "server" {
                            d = d.with(Dirs::CLIENT);
                        }
                        directions = Some(d);
                    } else {
                        side = Some(match lower.as_str() {
                            "server" => Side::Server,
                            "client" => Side::Client,
                            _ => Side::Shared,
                        });
                    }
                }
                "logical" => logical = true,
                "key" => {
                    let refs: Option<Vec<usize>> = args_text
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(|s| reference(s, false, &mut errors))
                        .collect();
                    if let Some(list) = refs.filter(|l| !l.is_empty()) {
                        key = Some(list);
                    }
                }
                "callback" => callback = reference(args_text, false, &mut errors),
                "inject" => match args_text.trim().parse::<usize>() {
                    Ok(n) => injected = n,
                    Err(_) => errors.push("inject() takes a number".into()),
                },
                "target" => target = reference(args_text, false, &mut errors),
                "payload" => payload = reference(args_text, true, &mut errors),
                "wire" => wire = reference(args_text, false, &mut errors),
                _ => {
                    if args.is_none() {
                        break;
                    }
                    errors.push(format!("Unknown clause '{word}'"));
                }
            }
        }
        let role = role?;
        let key = key.unwrap_or_else(|| match (role, params) {
            // unknown parameters: every argument but the network name is a key part
            (Role::Alias, None) => Vec::new(),
            (Role::Alias, Some(_)) => {
                let w = wire.unwrap_or(0);
                let all: Vec<usize> = (0..names.len()).filter(|&i| i != w).collect();
                if all.is_empty() { vec![1] } else { all }
            }
            _ => vec![0],
        });
        if role == Role::Alias && wire.is_none() {
            wire = Some(0);
        }
        let mut used: Vec<usize> = key.clone();
        let first_free =
            |used: &[usize]| (0..).find(|i| !used.contains(i) && Some(*i) != target && Some(*i) != wire).unwrap_or(0);
        if role == Role::Handler && callback.is_none() {
            let free = first_free(&used);
            callback = (free < names.len() || params.is_none()).then_some(free);
        }
        if role == Role::Trigger && payload.is_none() {
            used.extend(target);
            let free = first_free(&used);
            payload = (free < names.len() || vararg || params.is_none()).then_some(free);
        }
        Some(Spec { role, key, logical, callback, injected, net, directions, side, target, payload, wire, errors })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn to_function(
        &self,
        path: &str,
        kind: FnKind,
        origin: Origin,
        file: Option<Rc<str>>,
        nth: u32,
        defined_side: Side,
        param_count: Option<usize>,
        vararg: bool,
    ) -> EventFn {
        let side = self.side.unwrap_or(defined_side);
        // the key of an alias is always the logical side of the pair
        let key = if self.logical || self.key.len() > 1 || self.role == Role::Alias {
            KeySpec::logical(&self.key)
        } else {
            KeySpec::wire(self.key.first().copied().unwrap_or(0))
        };
        let directions = match self.role {
            Role::Trigger => self.directions.unwrap_or(match side {
                Side::Client => Dirs::SERVER,
                Side::Server => Dirs::CLIENT,
                _ => Dirs::SERVER.with(Dirs::CLIENT),
            }),
            _ => Dirs::NONE,
        };
        EventFn {
            path: path.to_owned(),
            role: self.role,
            key,
            origin,
            callback: self.callback,
            callback_optional: false,
            injected: self.injected,
            net: self.role == Role::Register || (self.role == Role::Handler && self.net),
            directions,
            target: self.target,
            payload_start: self.payload,
            payload_fixed: None,
            wire: self.wire,
            file,
            nth,
            kind,
            defined_side: side,
            redefinition: kind == FnKind::Global && is_standard_name(path),
            required_side: if self.role == Role::Trigger { directions.required_side() } else { None },
            param_count,
            vararg,
        }
    }
}

fn side_word(side: Side) -> Option<&'static str> {
    match side {
        Side::Client => Some("client"),
        Side::Server => Some("server"),
        Side::Shared => Some("both"),
        _ => None,
    }
}

pub fn format(f: &EventFn) -> String {
    let mut words: Vec<String> = Vec::new();
    match f.role {
        Role::Handler => {
            words.push("handler".into());
            words.push(if f.net { "net" } else { "local" }.into());
            words.extend(side_word(f.defined_side).map(String::from));
        }
        Role::Trigger => {
            words.push("trigger".into());
            let d = f.directions;
            if d == Dirs::LOCAL {
                words.push("local".into());
            } else if d == Dirs::SERVER {
                words.push("server".into());
            } else if d == Dirs::CLIENT {
                words.push("client".into());
            } else if d.has(Dirs::SERVER) && d.has(Dirs::CLIENT) {
                words.push("both".into());
            }
        }
        Role::Register => {
            words.push("register".into());
            words.extend(side_word(f.defined_side).map(String::from));
        }
        Role::Alias => words.push("alias".into()),
    }
    if f.key.logical && f.key.components.len() == 1 {
        words.push("logical".into());
    }
    if f.role == Role::Alias {
        words.push(format!("wire({})", f.wire.unwrap_or(0) + 1));
    }
    if !f.key.components.is_empty() {
        let comps: Vec<String> = f
            .key
            .components
            .iter()
            .map(|c| {
                c.iter()
                    .map(|s| match s {
                        Seg::Param(i) => (i + 1).to_string(),
                        Seg::Lit(t) => format!("'{t}'"),
                    })
                    .collect::<Vec<_>>()
                    .join(" .. ")
            })
            .collect();
        words.push(format!("key({})", comps.join(", ")));
    }
    if let Some(c) = f.callback {
        words.push(format!("callback({})", c + 1));
    }
    if f.injected > 0 {
        words.push(format!("inject({})", f.injected));
    }
    if let Some(t) = f.target {
        words.push(format!("target({})", t + 1));
    }
    if let Some(start) = f.payload_start {
        let vararg_payload = f.payload_fixed.is_none() && f.vararg && Some(start) == f.param_count;
        words.push(if vararg_payload { "payload(...)".into() } else { format!("payload({})", start + 1) });
    }
    words.join(" ")
}

pub fn settings_helper(line: &str) -> Option<EventFn> {
    let line = line.trim();
    let split = line.find(char::is_whitespace)?;
    let path = line[..split].trim_end_matches(':');
    if path.is_empty() {
        return None;
    }
    let spec = Spec::parse(&line[split..], None, true)?;
    Some(spec.to_function(path, FnKind::of_path(path), Origin::Settings, None, 0, Side::Unknown, None, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn grammar() {
        let p = names(&["player", "type", "name"]);
        let spec = Spec::parse("trigger client key(type, name) target(player) payload(...)", Some(&p), true).unwrap();
        assert_eq!(
            (spec.role, spec.key.clone(), spec.target, spec.payload),
            (Role::Trigger, vec![1, 2], Some(0), Some(3))
        );
        assert!(spec.errors.is_empty());
        let local =
            Spec::parse("handler local key(1) Registers a local event.", Some(&names(&["name", "cb"])), false).unwrap();
        assert!(!local.net);
        assert_eq!(local.callback, Some(1));
        let bad = Spec::parse("handler key(nope)", Some(&names(&["a"])), false).unwrap();
        assert_eq!(bad.errors, ["Unknown parameter 'nope'"]);
        assert!(Spec::parse("listener key(1)", Some(&names(&["a"])), false).is_none());
        let errors = Spec::parse("trigger key(0) callback(...) inject(x) frob(1)", None, true).unwrap().errors;
        assert_eq!(
            errors,
            [
                "Parameter positions start at 1",
                "'...' is only valid in payload()",
                "inject() takes a number",
                "Unknown clause 'frob'"
            ]
        );
        assert_eq!(
            Spec::parse("trigger payload(...)", None, true).unwrap().errors,
            ["'...' needs the function's parameters: use a position"]
        );
    }

    #[test]
    fn functions_and_format() {
        let p = names(&["eType", "eName", "cb"]);
        let spec = Spec::parse("handler server key(eType, eName) callback(cb) inject(1)", Some(&p), false).unwrap();
        let f = spec.to_function(
            "SrvHelper.OnBridge",
            FnKind::Field,
            Origin::Annotation,
            None,
            0,
            Side::Server,
            Some(3),
            false,
        );
        assert_eq!(format(&f), "handler net server key(1, 2) callback(3) inject(1)");
        let t = Spec::parse("trigger client key(1) payload(...)", Some(&names(&["name"])), true).unwrap();
        let f = t.to_function("SrvHelper.C", FnKind::Field, Origin::Annotation, None, 0, Side::Server, Some(1), true);
        assert_eq!(format(&f), "trigger client key(1) payload(...)");
        assert_eq!(f.required_side, Some(Side::Server));
        assert_eq!(format(&EventFn::alias("RegisterEventAlias")), "alias wire(1)");
        let legacy = settings_helper("Legacy.Emit trigger server key(1)").unwrap();
        assert_eq!(
            (legacy.path.as_str(), legacy.payload_start, legacy.origin),
            ("Legacy.Emit", Some(1), Origin::Settings)
        );
        let srv = settings_helper("SrvHelper.AddEventHandler: handler net server key(1, 2) callback(3)").unwrap();
        assert_eq!(format(&srv), "handler net server key(1, 2) callback(3)");
        assert!(settings_helper("NoSpec").is_none());
    }
}
