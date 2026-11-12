//! The calls of a file, packed: a database reads few of them, and by the name of the function called.

use super::flow::{Arg, CallRecord, CallbackCall};
use super::model::{Seg, Sources, Span, simple_name};
use openidea_plugin::{Position, Range};
use std::collections::BTreeMap;

const COLON: u8 = 1;
const ORIGINAL: u8 = 2;
const LAST_ARG_MULTI: u8 = 4;
const LOCAL_RECEIVER: u8 = 8;
const IN_FUNCTION: u8 = 16;
const RECEIVER_CLASS: u8 = 32;

const STR: u8 = 0;
const LITERAL: u8 = 1;
const TEMPLATE: u8 = 2;
const TAINTED: u8 = 3;
const CLOSURE: u8 = 4;
const FUNCTION_REF: u8 = 5;
const SPREAD: u8 = 6;
const DYNAMIC: u8 = 7;

const TEXT: u8 = 0;
const PARAM: u8 = 1;

const VARARG: u8 = 1;
const CALLBACK: u8 = 2;
const CLOSURE_VARARG: u8 = 4;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Calls {
    /// Per function called, sorted: the hash of its simple name and where its calls start in `bytes`.
    groups: Box<[(u32, u32)]>,
    bytes: Box<[u8]>,
}

fn hash_of(name: &str) -> u32 {
    name.bytes().fold(0x811c_9dc5, |hash, byte| (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193))
}

impl Calls {
    pub fn pack(calls: &[CallRecord]) -> Self {
        let mut by_callee: BTreeMap<(u32, &str), Vec<usize>> = BTreeMap::new();
        for (nth, call) in calls.iter().enumerate() {
            by_callee.entry((hash_of(call.simple_name()), call.callee.as_str())).or_default().push(nth);
        }
        let mut bytes = Vec::new();
        let mut groups = Vec::with_capacity(by_callee.len());
        for ((hash, callee), places) in by_callee {
            groups.push((hash, bytes.len() as u32));
            put_text(&mut bytes, callee);
            put(&mut bytes, places.len() as u64);
            for nth in places {
                put(&mut bytes, nth as u64);
                put_call(&mut bytes, &calls[nth]);
            }
        }
        Self { groups: groups.into(), bytes: bytes.into() }
    }

    /// The calls of the functions whose simple name is wanted, in the order of the file.
    pub fn of(&self, wanted: impl Fn(&str) -> bool) -> Vec<CallRecord> {
        self.read(self.groups.iter(), wanted)
    }

    pub fn named(&self, name: &str) -> Vec<CallRecord> {
        let hash = hash_of(name);
        let from = self.groups.partition_point(|(other, _)| *other < hash);
        self.read(self.groups[from..].iter().take_while(|(other, _)| *other == hash), |simple| simple == name)
    }

    pub fn all(&self) -> Vec<CallRecord> {
        self.of(|_| true)
    }

    fn read<'a>(&self, groups: impl Iterator<Item = &'a (u32, u32)>, wanted: impl Fn(&str) -> bool) -> Vec<CallRecord> {
        let mut found: Vec<(u64, CallRecord)> = Vec::new();
        let mut read = 0;
        for (_, at) in groups {
            let mut reader = Reader { bytes: &self.bytes, at: *at as usize };
            let Some(callee) = reader.text().filter(|callee| wanted(simple_name(callee))) else { continue };
            read += 1;
            reader.group(callee, &mut found);
        }
        if read > 1 {
            found.sort_by_key(|(nth, _)| *nth);
        }
        found.into_iter().map(|(_, call)| call).collect()
    }

    #[cfg(test)]
    pub fn size(&self) -> usize {
        self.groups.len() * std::mem::size_of::<(u32, u32)>() + self.bytes.len()
    }
}

fn put(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(value as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn put_signed(out: &mut Vec<u8>, value: i64) {
    put(out, ((value << 1) ^ (value >> 63)) as u64);
}

fn put_text(out: &mut Vec<u8>, text: &str) {
    put(out, text.len() as u64);
    out.extend_from_slice(text.as_bytes());
}

fn put_sources(out: &mut Vec<u8>, sources: &Sources) {
    put(out, sources.len() as u64);
    sources.iter().for_each(|source| put_signed(out, i64::from(*source)));
}

/// Written from where its call starts: a span stands close to it.
fn put_span(out: &mut Vec<u8>, span: &Span, call: &CallRecord) {
    put_signed(out, i64::from(span.start) - i64::from(call.offset));
    put_signed(out, i64::from(span.end) - i64::from(span.start));
    put_signed(out, i64::from(span.range.start.line) - i64::from(call.line));
    put(out, u64::from(span.range.start.character));
    put_signed(out, i64::from(span.range.end.line) - i64::from(span.range.start.line));
    put(out, u64::from(span.range.end.character));
}

fn put_call(out: &mut Vec<u8>, call: &CallRecord) {
    let flags = [
        (call.colon, COLON),
        (call.original, ORIGINAL),
        (call.last_arg_multi, LAST_ARG_MULTI),
        (call.local_receiver, LOCAL_RECEIVER),
        (call.function.is_some(), IN_FUNCTION),
        (call.receiver_class.is_some(), RECEIVER_CLASS),
    ];
    out.push(flags.iter().filter(|(set, _)| *set).fold(0, |all, (_, flag)| all | flag));
    put(out, u64::from(call.offset));
    put(out, u64::from(call.line));
    put_span(out, &call.callee_span, call);
    if let Some(function) = call.function {
        put(out, u64::from(function));
    }
    if let Some(class) = &call.receiver_class {
        put_text(out, class);
    }
    put(out, call.args.len() as u64);
    call.args.iter().for_each(|arg| put_arg(out, arg, call));
}

fn put_arg(out: &mut Vec<u8>, arg: &Arg, call: &CallRecord) {
    match arg {
        Arg::Str { value, lit: None } => {
            out.push(STR);
            put_text(out, value);
        }
        Arg::Str { value, lit: Some(span) } => {
            out.push(LITERAL);
            put_text(out, value);
            put_span(out, span, call);
        }
        Arg::Template(segs) => {
            out.push(TEMPLATE);
            put(out, segs.len() as u64);
            for seg in segs {
                match seg {
                    Seg::Lit(text) => {
                        out.push(TEXT);
                        put_text(out, text);
                    }
                    Seg::Param(nth) => {
                        out.push(PARAM);
                        put(out, *nth as u64);
                    }
                }
            }
        }
        Arg::Tainted(sources) => {
            out.push(TAINTED);
            put_sources(out, sources);
        }
        Arg::Closure { params, vararg, callback } => {
            out.push(CLOSURE);
            put(out, *params as u64);
            let closure_vararg = callback.as_ref().is_some_and(|callback| callback.closure_vararg);
            let flags = [(*vararg, VARARG), (callback.is_some(), CALLBACK), (closure_vararg, CLOSURE_VARARG)];
            out.push(flags.iter().filter(|(set, _)| *set).fold(0, |all, (_, flag)| all | flag));
            if let Some(callback) = callback {
                put(out, callback.param as u64);
                put(out, callback.closure_params as u64);
                put(out, callback.inputs.len() as u64);
                callback.inputs.iter().for_each(|sources| put_sources(out, sources));
            }
        }
        Arg::FunctionRef { params, vararg } => {
            out.push(FUNCTION_REF);
            put(out, *params as u64);
            out.push(u8::from(*vararg));
        }
        Arg::Spread(sources) => {
            out.push(SPREAD);
            put_sources(out, sources);
        }
        Arg::Dynamic(prefix) => {
            out.push(DYNAMIC);
            put_text(out, prefix);
        }
    }
}

/// Reads what `pack` wrote; bytes it did not write give nothing, never a panic.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn byte(&mut self) -> Option<u8> {
        let byte = *self.bytes.get(self.at)?;
        self.at += 1;
        Some(byte)
    }

    fn number(&mut self) -> Option<u64> {
        let mut value = 0;
        for shift in (0..64).step_by(7) {
            let byte = self.byte()?;
            value |= u64::from(byte & 0x7f) << shift;
            if byte < 0x80 {
                return Some(value);
            }
        }
        None
    }

    fn signed(&mut self) -> Option<i64> {
        let value = self.number()?;
        Some((value >> 1) as i64 ^ -((value & 1) as i64))
    }

    fn small(&mut self) -> Option<u32> {
        u32::try_from(self.number()?).ok()
    }

    fn size(&mut self) -> Option<usize> {
        usize::try_from(self.number()?).ok()
    }

    fn near(&mut self, base: u32) -> Option<u32> {
        u32::try_from(i64::from(base).checked_add(self.signed()?)?).ok()
    }

    /// How many items follow: each takes a byte at least.
    fn count(&mut self) -> Option<usize> {
        self.size().filter(|count| *count <= self.bytes.len() - self.at)
    }

    fn text(&mut self) -> Option<&'a str> {
        let len = self.count()?;
        let text = std::str::from_utf8(&self.bytes[self.at..self.at + len]).ok()?;
        self.at += len;
        Some(text)
    }

    fn sources(&mut self) -> Option<Sources> {
        let count = self.count()?;
        (0..count).map(|_| i32::try_from(self.signed()?).ok()).collect()
    }

    fn span(&mut self, offset: u32, line: u32) -> Option<Span> {
        let start = self.near(offset)?;
        let end = self.near(start)?;
        let first = Position::new(self.near(line)?, self.small()?);
        let last = Position::new(self.near(first.line)?, self.small()?);
        Some(Span { start, end, range: Range::new(first, last) })
    }

    fn group(&mut self, callee: &str, found: &mut Vec<(u64, CallRecord)>) -> Option<()> {
        for _ in 0..self.count()? {
            let nth = self.number()?;
            found.push((nth, self.call(callee)?));
        }
        Some(())
    }

    fn call(&mut self, callee: &str) -> Option<CallRecord> {
        let flags = self.byte()?;
        let has = |flag: u8| flags & flag != 0;
        let offset = self.small()?;
        let line = self.small()?;
        let callee_span = self.span(offset, line)?;
        let function = if has(IN_FUNCTION) { Some(self.small()?) } else { None };
        let receiver_class = if has(RECEIVER_CLASS) { Some(self.text()?.to_owned()) } else { None };
        let count = self.count()?;
        let mut args = Vec::with_capacity(count);
        for _ in 0..count {
            args.push(self.arg(offset, line)?);
        }
        Some(CallRecord {
            callee: callee.to_owned(),
            colon: has(COLON),
            original: has(ORIGINAL),
            offset,
            line,
            callee_span,
            args,
            last_arg_multi: has(LAST_ARG_MULTI),
            function,
            local_receiver: has(LOCAL_RECEIVER),
            receiver_class,
        })
    }

    fn arg(&mut self, offset: u32, line: u32) -> Option<Arg> {
        Some(match self.byte()? {
            STR => Arg::Str { value: self.text()?.to_owned(), lit: None },
            LITERAL => Arg::Str { value: self.text()?.to_owned(), lit: Some(self.span(offset, line)?) },
            TEMPLATE => {
                let count = self.count()?;
                let mut segs = Vec::with_capacity(count);
                for _ in 0..count {
                    segs.push(match self.byte()? {
                        TEXT => Seg::Lit(self.text()?.to_owned()),
                        PARAM => Seg::Param(self.size()?),
                        _ => return None,
                    });
                }
                Arg::Template(segs)
            }
            TAINTED => Arg::Tainted(self.sources()?),
            CLOSURE => {
                let params = self.size()?;
                let flags = self.byte()?;
                let callback = if flags & CALLBACK == 0 {
                    None
                } else {
                    let (param, closure_params) = (self.size()?, self.size()?);
                    let count = self.count()?;
                    let inputs = (0..count).map(|_| self.sources()).collect::<Option<Vec<Sources>>>()?;
                    Some(CallbackCall { param, closure_params, closure_vararg: flags & CLOSURE_VARARG != 0, inputs })
                };
                Arg::Closure { params, vararg: flags & VARARG != 0, callback }
            }
            FUNCTION_REF => Arg::FunctionRef { params: self.size()?, vararg: self.byte()? != 0 },
            SPREAD => Arg::Spread(self.sources()?),
            DYNAMIC => Arg::Dynamic(self.text()?.to_owned()),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::flow::Analysis;
    use crate::lua::Source;

    const HELPERS: &str = "local PREFIX = 'shop:'\nlocal _Trigger = TriggerServerEvent\n\
        function Events.Send(category, name, ...)\n\
            TriggerServerEvent('evt:' .. category .. ':' .. name, ...)\n\
            _Trigger(PREFIX, 1)\n\
            B.Trigger(name, table.unpack({ category }))\n\
        end\n\
        function Events.On(eType, eName, callback)\n\
            A.Trigger(eType .. eName)\n\
            AddEventHandler(GetName(eType), function(actor, ...)\n\
                callback(actor, ...)\n\
            end)\n\
        end\n\
        local function handle() end\n\
        local zone = Zone.new('x')\nzone:TriggerEvent('a', [[b\nc]])\n\
        A.Trigger('one', handle)\nB.Trigger('two', function(a, ...) end)\nA.Trigger('three')\n\
        TriggerServerEvent('shop:' .. GetCurrentResourceName())\nTriggerServerEvent(GetValues())";

    fn calls_of(text: &str) -> Vec<CallRecord> {
        let src = Source::new(text);
        let ast = crate::events::ast::parse(&src);
        Analysis::new(&src, &ast).calls()
    }

    #[test]
    fn the_calls_read_back_are_the_calls_packed() {
        let calls = calls_of(HELPERS);
        let kinds: Vec<u8> = calls
            .iter()
            .flat_map(|call| &call.args)
            .map(|arg| {
                let mut out = Vec::new();
                put_arg(&mut out, arg, &calls[0]);
                out[0]
            })
            .collect();
        for kind in [STR, LITERAL, TEMPLATE, TAINTED, CLOSURE, FUNCTION_REF, SPREAD, DYNAMIC] {
            assert!(kinds.contains(&kind), "the text has no argument of the kind {kind}");
        }
        assert!(calls.iter().any(|call| call.receiver_class.is_some() && call.colon && call.local_receiver));
        assert!(calls.iter().any(|call| call.original) && calls.iter().any(|call| call.last_arg_multi));
        assert!(calls.iter().any(|call| matches!(&call.args[..], [.., Arg::Closure { callback: Some(_), .. }])));

        let packed = Calls::pack(&calls);
        assert_eq!(packed.all(), calls);
        assert_eq!(packed, Calls::pack(&calls));
        assert_eq!(Calls::pack(&[]), Calls::default());
        assert_eq!(Calls::default().all(), []);
    }

    #[test]
    fn the_calls_of_a_name_come_in_the_order_of_the_file() {
        let calls = calls_of(HELPERS);
        let packed = Calls::pack(&calls);
        let named = |name: &str| -> Vec<CallRecord> {
            calls.iter().filter(|call| call.simple_name() == name).cloned().collect()
        };
        let callees: Vec<String> = packed.named("Trigger").into_iter().map(|call| call.callee).collect();
        assert_eq!(callees, ["B.Trigger", "A.Trigger", "A.Trigger", "B.Trigger", "A.Trigger"]);
        for name in ["Trigger", "TriggerServerEvent", "TriggerEvent", "AddEventHandler"] {
            assert_eq!(packed.named(name), named(name), "{name}");
        }
        assert_eq!(packed.named("Trigge"), []);
        assert_eq!(packed.named("A.Trigger"), [], "a simple name is asked for");

        let wanted = ["Trigger", "AddEventHandler"];
        let both: Vec<CallRecord> = calls.iter().filter(|call| wanted.contains(&call.simple_name())).cloned().collect();
        assert_eq!(packed.of(|name| wanted.contains(&name)), both);
    }

    #[test]
    fn another_call_packs_into_other_bytes() {
        let calls = calls_of(HELPERS);
        let packed = Calls::pack(&calls);
        for nth in 0..calls.len() {
            let mut other = calls.clone();
            other[nth].line += 1;
            assert_ne!(Calls::pack(&other), packed);
            let mut fewer = calls.clone();
            fewer.remove(nth);
            assert_ne!(Calls::pack(&fewer), packed);
        }
    }

    #[test]
    fn bytes_that_were_not_packed_give_no_panic() {
        let calls = calls_of(HELPERS);
        let packed = Calls::pack(&calls);
        for len in 0..packed.bytes.len() {
            let cut = Calls { groups: packed.groups.clone(), bytes: packed.bytes[..len].into() };
            assert!(cut.all().len() < calls.len(), "cut at {len}");
        }
        for at in 0..packed.bytes.len() {
            for byte in [0x00, 0x7f, 0x80, 0xff] {
                let mut bytes = packed.bytes.clone();
                bytes[at] = byte;
                let changed = Calls { groups: packed.groups.clone(), bytes };
                // A count that grew reads the calls that follow it: never more than the bytes hold.
                let most = packed.groups.len() * packed.bytes.len();
                assert!(changed.all().len() <= most && changed.named("Trigger").len() <= most);
            }
        }
    }
}
