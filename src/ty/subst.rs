use super::{Class, Function, Generic, Param, Ret, Signature, Ty};
use std::rc::Rc;

impl Ty {
    /// The type with its classes rewritten: `replace` gives what stands for a class, or `None` to keep it.
    /// What the body of a function of the code returns is left as it is.
    pub fn substitute(&self, replace: &mut dyn FnMut(&Class) -> Option<Ty>) -> Ty {
        match self {
            Ty::Class(class) => replace(class).unwrap_or_else(|| self.clone()),
            Ty::Array(base) => Ty::array(base.substitute(replace)),
            Ty::Tuple(list) => Ty::tuple(list.iter().map(|ty| ty.substitute(replace)).collect()),
            Ty::Union(members) => {
                let mut out = Ty::Unknown;
                for member in members.iter() {
                    out.add(&member.substitute(replace));
                }
                out
            }
            Ty::Generic(generic) => Ty::Generic(Rc::new(Generic {
                base: generic.base.substitute(replace),
                params: generic.params.iter().map(|param| param.substitute(replace)).collect(),
            })),
            Ty::Function(function) => Ty::Function(Rc::new(Function {
                main: function.main.substitute(replace),
                overloads: function.overloads.iter().map(|signature| signature.substitute(replace)).collect(),
                flags: function.flags,
            })),
            _ => self.clone(),
        }
    }
}

impl Signature {
    pub fn substitute(&self, replace: &mut dyn FnMut(&Class) -> Option<Ty>) -> Signature {
        Signature {
            colon_call: self.colon_call,
            params: self
                .params
                .iter()
                .map(|param| Param { ty: param.ty.substitute(replace), ..param.clone() })
                .collect(),
            ret: match &self.ret {
                Ret::Ty(ty) => Ret::Ty(ty.substitute(replace)),
                Ret::Body(func) => Ret::Body(*func),
            },
            vararg: self.vararg.as_ref().map(|ty| ty.substitute(replace)),
            generics: self.generics.clone(),
        }
    }
}

/// What each generic parameter of a signature stands for in one call.
pub type Bindings = Vec<(Rc<str>, Ty)>;

struct Analyzer<'a> {
    bindings: &'a mut Bindings,
    ret: &'a dyn Fn(&Ret) -> Ty,
}

impl Analyzer<'_> {
    fn visit(&mut self, given: &Ty, declared: &Ty) {
        match declared {
            Ty::Class(class) => {
                if let Some((_, bound)) = self.bindings.iter_mut().find(|(name, _)| *name == class.name) {
                    bound.add(given);
                }
            }
            Ty::Union(list) | Ty::Tuple(list) => {
                for member in list.iter() {
                    self.visit(given, member);
                }
            }
            Ty::Array(base) => {
                for ty in given.each() {
                    if let Ty::Array(given) = ty {
                        self.enter(given, base);
                    }
                }
            }
            Ty::Function(function) => {
                for ty in given.each() {
                    if let Ty::Function(given) = ty {
                        self.enter(&(self.ret)(&given.main.ret), &(self.ret)(&function.main.ret));
                    }
                }
            }
            Ty::Generic(generic) => {
                for ty in given.each() {
                    let Ty::Generic(given) = ty else { continue };
                    self.enter(&given.base, &generic.base);
                    for (param, declared) in given.params.iter().zip(&generic.params) {
                        self.enter(param, declared);
                    }
                }
            }
            _ => {}
        }
    }

    fn enter(&mut self, given: &Ty, declared: &Ty) {
        if !given.is_invalid() {
            self.visit(given, declared);
        }
    }
}

/// Reads what the generic parameters named in `bindings` stand for, from the type `given` to a parameter
/// declared as `declared` (`GenericAnalyzer`): `T[]` given `Emmy[]` binds `T` to `Emmy`. `ret` gives what a
/// signature returns.
pub fn analyze(given: &Ty, declared: &Ty, bindings: &mut Bindings, ret: &dyn Fn(&Ret) -> Ty) {
    Analyzer { bindings, ret }.enter(given, declared);
}
