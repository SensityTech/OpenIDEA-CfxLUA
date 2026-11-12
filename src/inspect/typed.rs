//! The type safety inspections of the JetBrains plugin, off unless they are asked for
//! (`MatchFunctionSignatureInspection`, `ReturnTypeInspection`, `AssignTypeInspection`,
//! `UndeclaredVariableInspection`).

use super::{ASSIGN_TYPE, Inspection, MATCH_SIGNATURE, RETURN_TYPE, UNDECLARED};
use crate::feature::Problem;
use crate::lua::Source;
use crate::lua::token::T;
use crate::lua::tree::{N, Node};
use crate::ty::context::{Cx, Psi};
use crate::ty::doc::Reader;
use crate::ty::expressions::{solved, values};
use crate::ty::subtype::{fits, is_uncheckable};
use crate::ty::{FileId, Function, G, SELF, Ty, display, members, psi, resolve};
use openidea_plugin::Severity;

struct Checking<'a, 'c> {
    cx: &'a Cx<'c>,
    file: FileId,
    src: &'a Source,
    out: Vec<Problem>,
}

impl<'a> Checking<'a, '_> {
    fn psi(&self, node: Node<'a>) -> Psi<'a> {
        Psi { file: self.file, src: self.src, node }
    }

    /// The type of the first value of an expression.
    fn single(&self, node: Node<'a>) -> Ty {
        self.cx.with_value(0, || self.cx.infer(self.psi(node)))
    }

    /// The type of the member `name` of an object whose type is made of classes: `Some(None)` when none of
    /// them declares it, `None` when the object is something else and nothing can be said.
    fn member_type(&self, object: &Ty, name: &str) -> Option<Option<Ty>> {
        let classes = object.top_classes();
        if classes.is_empty() || classes.len() != object.each().len() {
            return None;
        }
        let member = classes.into_iter().find_map(|class| members::find(self.cx, class, name));
        Some(member.map(|member| members::type_of(self.cx, &member)))
    }

    fn report(&mut self, inspection: Inspection, (start, end): (usize, usize), message: String) {
        self.out.push(Problem::new(inspection.code, Severity::Warning, start, end, message));
    }

    /// Each value a call is given, with the argument that gives it: the last argument gives all its values,
    /// any other one its first.
    fn given(&self, args: &[Node<'a>]) -> Vec<(Node<'a>, Ty)> {
        let mut given = Vec::with_capacity(args.len());
        for (rank, arg) in args.iter().enumerate() {
            match self.cx.with_value(-1, || self.cx.infer(self.psi(*arg))) {
                Ty::Tuple(list) if rank + 1 == args.len() => given.extend(list.iter().map(|ty| (*arg, ty.clone()))),
                Ty::Tuple(list) => given.push((*arg, list.first().cloned().unwrap_or(Ty::Unknown))),
                ty => given.push((*arg, ty)),
            }
        }
        given
    }

    /// The arguments of a call against the signature that takes that many (`annotateCall`).
    fn arguments(&mut self, call: Node<'a>, callee: Node<'a>, function: &Function) {
        let args = psi::args(call);
        let colon = psi::is_colon_call(call);
        let dot = callee.is(N::IndexExpr) && !colon;
        let counted = match (function.is_colon_call() && dot, !function.is_colon_call() && colon) {
            (true, _) => args.len().saturating_sub(1),
            (_, true) => args.len() + 1,
            _ => args.len(),
        };
        let signature = solved(self.cx, self.psi(call), function.perfect_signature(counted));
        let given = self.given(&args);
        let params = signature.args(None, colon);
        for (rank, param) in params.iter().enumerate() {
            match given.get(rank) {
                None if param.optional => {}
                None => {
                    let closing = call.children().find(|child| matches!(child.kind(), N::ListArgs | N::SingleArg));
                    let at =
                        closing.and_then(|list| list.last_token()).map_or(call.code_range(), |token| token.range());
                    let message = format!("Missing argument: {}: {}", param.name, display(&param.ty));
                    self.report(MATCH_SIGNATURE, at, message);
                }
                Some((arg, ty)) => {
                    if !fits(self.cx, ty, &param.ty, false) && !is_uncheckable(self.cx, &param.ty, ty) {
                        let message =
                            format!("Type mismatch. Required: '{}' Found: '{}'", display(&param.ty), display(ty));
                        self.report(MATCH_SIGNATURE, arg.code_range(), message);
                    }
                }
            }
        }
        if signature.vararg.is_none() {
            for extra in args.iter().skip(params.len()) {
                self.report(MATCH_SIGNATURE, extra.code_range(), "Too many arguments.".to_owned());
            }
        }
    }

    /// A call: what it calls is a function that takes what it is given (`MatchFunctionSignatureInspection`).
    fn call(&mut self, call: Node<'a>) {
        let Some(callee) = psi::prefix(call) else { return };
        let named = callee.token(T::Id).map(|id| id.text());
        if callee.is(N::IndexExpr) && psi::is_colon(callee) && named.is_some() {
            for object in callee.exprs().filter(|object| self.single(*object) == Ty::Nil).collect::<Vec<_>>() {
                self.report(MATCH_SIGNATURE, object.code_range(), "Trying to index a nil type.".to_owned());
            }
        }
        let ty = self.single(callee);
        let unknown = |name: Option<&str>| format!("Unknown function '{}'.", name.unwrap_or_else(|| callee.code()));
        match &ty {
            Ty::Function(function) => self.arguments(call, callee, function),
            _ if callee.is(N::IndexExpr) => {
                let object = psi::prefix(callee).map_or(Ty::Unknown, |object| self.single(object));
                if named.is_some_and(|name| self.member_type(&object, name) == Some(None)) {
                    self.report(MATCH_SIGNATURE, call.code_range(), unknown(named));
                }
            }
            Ty::Nil => self.report(MATCH_SIGNATURE, call.code_range(), unknown(named)),
            _ => {}
        }
    }

    /// A name nothing declares: no local, no global of the project or of a library, no native
    /// (`UndeclaredVariableInspection`).
    fn name(&mut self, name: Node<'a>) {
        let text = name.code();
        let here = self.psi(name);
        let known = self.src.scopes().resolve(name).is_some()
            || matches!(text, G | "_ENV")
            || (text == SELF && resolve::self_owner(name).is_some())
            || !members::globals(self.cx, here, text).is_empty()
            || self.cx.files.native(text).is_some();
        if !known {
            self.report(UNDECLARED, name.code_range(), format!("Undeclared variable '{text}'."));
        }
    }

    /// What a function says it returns: its `@return` tags, or for a method marked `@override` what the
    /// method it overrides returns (`guessSuperReturnTypes`).
    fn declared_returns(&self, function: Node<'a>) -> Vec<Ty> {
        // A function that is a value has no documentation of its own.
        let owner = Some(function).filter(|function| function.kind().is_statement());
        let Some(comment) = owner.and_then(|owner| self.src.comment_of(owner)) else { return Vec::new() };
        let declared = match function.is(N::ClassMethodDef) && comment.has_tag_named("override") {
            true => self.overridden_returns(function),
            false => Reader::new(self.file).returns(comment),
        };
        match declared {
            Some(Ty::Tuple(list)) => list.to_vec(),
            Some(ty) => vec![ty],
            None => Vec::new(),
        }
    }

    fn overridden_returns(&self, method: Node<'a>) -> Option<Ty> {
        let name = psi::function_name(method)?;
        let class = self.single(psi::method_owner(method)?);
        let parents = members::supers(self.cx, class.perfect_class()?);
        let inherited = parents.iter().find_map(|parent| members::find(self.cx, &Ty::named(parent), name))?;
        match members::type_of(self.cx, &inherited) {
            Ty::Function(function) => Some(self.cx.ret(&function.main.ret)),
            _ => None,
        }
    }

    /// A `return` gives what its function says it returns (`ReturnTypeInspection`).
    fn returned(&mut self, statement: Node<'a>) {
        let Some(function) = psi::enclosing_function(statement) else { return };
        let mut declared = self.declared_returns(function);
        let given = match statement.child(N::ExprList) {
            Some(list) => match self.cx.with_value(-1, || values(self.cx, self.psi(list))) {
                Ty::Tuple(list) => list.to_vec(),
                ty => vec![ty],
            },
            None => Vec::new(),
        };
        declared.resize(declared.len().max(given.len()), Ty::Unknown);
        let at = statement.last_token().map_or(statement.code_range(), |token| token.range());
        if declared.len() > given.len() {
            let message = match given.is_empty() {
                true => format!("Type mismatch. Expected: '{}' Found: 'nil'", display(&declared[0])),
                false => {
                    format!("Incorrect number of return values. Expected {} but found {}.", declared.len(), given.len())
                }
            };
            return self.report(RETURN_TYPE, at, message);
        }
        if given.iter().zip(&declared).any(|(given, declared)| !fits(self.cx, given, declared, false)) {
            let shown = |types: &[Ty]| types.iter().map(display).collect::<Vec<_>>().join(", ");
            let message = format!("Type mismatch. Expected: '{}' Found: '{}'", shown(&declared), shown(&given));
            self.report(RETURN_TYPE, statement.code_range(), message);
        }
    }

    /// A function that says what it returns and never returns.
    fn never_returns(&mut self, function: Node<'a>) {
        let Some(body) = function.child(N::FuncBody) else { return };
        if function.is(N::ClosureExpr) || body.descendants().any(|node| node.is(N::ReturnStat)) {
            return;
        }
        let declared = self.declared_returns(function);
        if !declared.is_empty() {
            let shown = declared.iter().map(display).collect::<Vec<_>>().join(",");
            let message = format!("Return type '{shown}' specified but no return values found.");
            self.report(RETURN_TYPE, body.code_range(), message);
        }
    }

    /// An assignment gives each of its targets a value of its type (`AssignTypeInspection`).
    fn assignment(&mut self, statement: Node<'a>) {
        if statement.has(T::In) {
            return;
        }
        let targets: Vec<Node> = statement.child(N::VarList).into_iter().flat_map(Node::exprs).collect();
        let given: Vec<Node> = statement.child(N::ExprList).into_iter().flat_map(Node::exprs).collect();
        for target in targets.iter().skip(given.len()) {
            self.report(ASSIGN_TYPE, target.code_range(), "Missing value assignment.".to_owned());
        }
        for value in given.iter().skip(targets.len()) {
            self.report(ASSIGN_TYPE, value.code_range(), "Nothing to assign to.".to_owned());
        }
        if targets.len() != given.len() {
            return;
        }
        let documented = self.src.comment_of(statement).is_some();
        for (target, value) in targets.iter().zip(&given) {
            let found = self.single(*value);
            let required = match target.kind() {
                N::IndexExpr => {
                    let object = psi::prefix(*target).map_or(Ty::Unknown, |object| self.single(object));
                    let Some(name) = psi::member_name(self.src, *target) else { continue };
                    match self.member_type(&object, &name) {
                        Some(member) => member.unwrap_or(Ty::Nil),
                        None => continue,
                    }
                }
                _ if documented => continue,
                _ => self.single(*target),
            };
            if !fits(self.cx, &found, &required, false) {
                let message = format!("Type mismatch. Required: '{}' Found: '{}'", display(&required), display(&found));
                self.report(ASSIGN_TYPE, value.code_range(), message);
            }
        }
    }
}

/// What the inspections `wanted` keeps find in `src`, the file `file`, in the order of the text.
pub fn problems(cx: &Cx, file: FileId, src: &Source, wanted: &dyn Fn(Inspection) -> bool) -> Vec<Problem> {
    let (signatures, undeclared, returns, assigns) =
        (wanted(MATCH_SIGNATURE), wanted(UNDECLARED), wanted(RETURN_TYPE), wanted(ASSIGN_TYPE));
    let mut checking = Checking { cx, file, src, out: Vec::new() };
    if !(signatures || undeclared || returns || assigns) {
        return checking.out;
    }
    for node in src.root().descendants() {
        match node.kind() {
            N::CallExpr if signatures => checking.call(node),
            N::NameExpr if undeclared => checking.name(node),
            N::ReturnStat if returns => checking.returned(node),
            N::AssignStat if assigns => checking.assignment(node),
            _ if returns && psi::is_function(node) => checking.never_returns(node),
            _ => {}
        }
    }
    checking.out.sort_by_key(|problem| problem.start);
    checking.out
}
