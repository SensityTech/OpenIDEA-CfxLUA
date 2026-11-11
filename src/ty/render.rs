use super::{ANONYMOUS_TABLE, Class, Detail, FuncRef, Primitive, Ret, Signature, Ty, literal_value};

/// How a type is written for the user (`TyRenderer`). A renderer overrides what it shows differently.
pub trait Render {
    /// What the body of a function of the code returns.
    fn body(&self, _func: FuncRef) -> Ty {
        Ty::Unknown
    }

    fn type_name(&self, name: &str) -> String {
        name.to_owned()
    }

    fn class(&self, class: &Class) -> String {
        if let Detail::Table(fields) = &class.detail {
            let fields: Vec<String> =
                fields.iter().map(|field| format!("{}: {}", field.name, self.render(&field.ty))).collect();
            return format!("{{ {} }}", fields.join(", "));
        }
        if class.flags & ANONYMOUS_TABLE != 0 {
            return self.type_name(Primitive::Table.name());
        }
        if class.flags & super::ANONYMOUS != 0 {
            return format!("[local {}]", class.var_name);
        }
        if class.flags & super::GLOBAL != 0 {
            return format!("[global {}]", class.var_name);
        }
        self.type_name(&class.name)
    }

    /// A string literal in quotes; `'"value"'`, the older notation, already holds its own.
    fn string_literal(&self, content: &str) -> String {
        if literal_value(content) == content { format!("\"{content}\"") } else { content.to_owned() }
    }

    fn ret(&self, ret: &Ret) -> Ty {
        match ret {
            Ret::Ty(ty) => ty.clone(),
            Ret::Body(func) => self.body(*func),
        }
    }

    fn signature(&self, signature: &Signature, out: &mut String) {
        let params: Vec<String> = signature
            .params
            .iter()
            .map(|param| format!("{}: {}", param.display_name(), self.render(&param.ty)))
            .collect();
        out.push_str(&format!("({}): ", params.join(", ")));
        self.write(&self.ret(&signature.ret), out);
    }

    fn render(&self, ty: &Ty) -> String {
        let mut out = String::new();
        self.write(ty, &mut out);
        out
    }

    fn write(&self, ty: &Ty, out: &mut String) {
        match ty {
            Ty::Unknown => out.push_str(&self.type_name("any")),
            Ty::Void => out.push_str(&self.type_name("void")),
            Ty::Nil => out.push_str(&self.type_name("nil")),
            Ty::Primitive(primitive) => out.push_str(&self.type_name(primitive.name())),
            Ty::Class(class) => out.push_str(&self.class(class)),
            Ty::StringLiteral(content) => out.push_str(&self.string_literal(content)),
            Ty::Array(base) => {
                self.write(base, out);
                out.push_str("[]");
            }
            Ty::Function(function) => {
                out.push_str("fun");
                self.signature(&function.main, out);
            }
            Ty::Generic(generic) => {
                let params: Vec<String> = generic.params.iter().map(|param| self.type_name(&display(param))).collect();
                out.push_str(&format!("{}<{}>", self.type_name(&display(&generic.base)), params.join(", ")));
            }
            Ty::Tuple(list) => {
                let list: Vec<String> = list.iter().map(|ty| self.render(ty)).collect();
                if list.len() > 1 {
                    out.push_str(&format!("({})", list.join(", ")));
                } else {
                    out.push_str(&list.join(", "));
                }
            }
            Ty::Union(members) => {
                let mut shown: Vec<String> = Vec::new();
                for member in members.iter() {
                    let text = self.render(member);
                    if !text.is_empty() && !shown.contains(&text) {
                        shown.push(text);
                    }
                }
                out.push_str(&if shown.is_empty() { "any".to_owned() } else { shown.join("|") });
            }
        }
    }
}

/// The plain renderer: the display name of a type.
pub struct Simple;

impl Render for Simple {}

/// The display name of a type; what a function of the code returns is left as `any`.
pub fn display(ty: &Ty) -> String {
    Simple.render(ty)
}
