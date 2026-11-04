#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    List,
    Vector,
    Keyed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoordText {
    pub components: Vec<String>,
    pub format: Format,
    pub lua_vector: bool,
}

impl CoordText {
    pub fn to_vector(&self) -> String {
        vector(if self.components.len() == 4 { "vector4" } else { "vector3" }, &self.components)
    }
}

pub const KEYS: [&str; 4] = ["x", "y", "z", "w"];

pub fn vector(constructor: &str, components: &[impl AsRef<str>]) -> String {
    format!("{constructor}({})", join(components))
}

pub fn keyed(components: &[impl AsRef<str>]) -> String {
    let fields: Vec<String> = components.iter().zip(KEYS).map(|(c, k)| format!("{k} = {}", c.as_ref())).collect();
    format!("{{ {} }}", fields.join(", "))
}

pub fn array(components: &[impl AsRef<str>]) -> String {
    format!("{{ {} }}", join(components))
}

pub fn join(components: &[impl AsRef<str>]) -> String {
    components.iter().map(AsRef::as_ref).collect::<Vec<_>>().join(", ")
}

const MAX_LENGTH: usize = 200;

pub fn parse(text: &str) -> Option<CoordText> {
    let core = text.trim();
    let core = core.strip_suffix(',').unwrap_or(core);
    let core = core.strip_suffix(';').unwrap_or(core).trim();
    if core.is_empty() || core.len() > MAX_LENGTH {
        return None;
    }
    parse_vector(core).or_else(|| parse_keyed(core)).or_else(|| parse_list(core))
}

fn normalize(number: &str) -> String {
    let t = number.trim();
    let t = t.strip_prefix('+').unwrap_or(t);
    t.strip_suffix(['f', 'F', 'd', 'D']).unwrap_or(t).to_owned()
}

fn number_end(b: &[u8], mut i: usize) -> Option<usize> {
    let digits = |b: &[u8], mut i: usize| {
        let s = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        (i, i - s)
    };
    if matches!(b.get(i), Some(b'-' | b'+')) {
        i += 1;
    }
    let (after, n) = digits(b, i);
    if n > 0 {
        i = after;
        if b.get(i) == Some(&b'.') {
            i = digits(b, i + 1).0;
        }
    } else if b.get(i) == Some(&b'.') {
        let (after, n) = digits(b, i + 1);
        if n == 0 {
            return None;
        }
        i = after;
    } else {
        return None;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(b.get(j), Some(b'-' | b'+')) {
            j += 1;
        }
        let (after, n) = digits(b, j);
        if n > 0 {
            i = after;
        }
    }
    if matches!(b.get(i), Some(b'f' | b'F' | b'd' | b'D')) {
        i += 1;
    }
    Some(i)
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while b.get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    i
}

fn arguments(s: &str, mut i: usize) -> Option<Vec<&str>> {
    let b = s.as_bytes();
    if b.get(i) != Some(&b'(') {
        return None;
    }
    let mut out = Vec::new();
    loop {
        i = skip_ws(b, i + 1);
        let end = number_end(b, i)?;
        out.push(&s[i..end]);
        i = skip_ws(b, end);
        match b.get(i) {
            Some(b',') if out.len() < 4 => {}
            Some(b')') if out.len() >= 3 && i + 1 == b.len() => return Some(out),
            _ => return None,
        }
    }
}

fn parse_vector(text: &str) -> Option<CoordText> {
    let lower = text.to_ascii_lowercase();
    let b = text.as_bytes();
    let mut i = 0;
    let mut new = false;
    if lower.starts_with("new") && b.get(3).is_some_and(u8::is_ascii_whitespace) {
        new = true;
        i = skip_ws(b, 3);
    }
    let name_start = i;
    i += if lower[i..].starts_with("vector") {
        6
    } else if lower[i..].starts_with("vec") {
        3
    } else {
        return None;
    };
    let size = match b.get(i) {
        Some(c @ (b'3' | b'4')) => {
            i += 1;
            Some(usize::from(c - b'0'))
        }
        _ => None,
    };
    let constructor = &text[name_start..i];
    let numbers = arguments(text, skip_ws(b, i))?;
    if size.is_some_and(|s| s != numbers.len()) {
        return None;
    }
    let lua_vector = !new
        && ["vector3", "vec3", "vector4", "vec4", "vector", "vec"].contains(&constructor)
        && numbers.iter().all(|n| !n.ends_with(|c: char| c.is_ascii_alphabetic()) && !n.starts_with('+'));
    Some(CoordText { components: numbers.into_iter().map(normalize).collect(), format: Format::Vector, lua_vector })
}

fn parse_keyed(text: &str) -> Option<CoordText> {
    let mut body = text;
    if body.starts_with('{') && body.ends_with('}') && body.len() >= 2 {
        body = &body[1..body.len() - 1];
    }
    let body = body.trim();
    if body.is_empty() {
        return None;
    }
    let b = body.as_bytes();
    let mut values: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        i = skip_ws(b, i);
        if b.get(i) == Some(&b'[') {
            i = skip_ws(b, i + 1);
        }
        if matches!(b.get(i), Some(b'"' | b'\'')) {
            i += 1;
        }
        let key_start = i;
        while b.get(i).is_some_and(u8::is_ascii_alphabetic) {
            i += 1;
        }
        if i == key_start {
            return None;
        }
        let mut key = body[key_start..i].to_ascii_lowercase();
        if matches!(b.get(i), Some(b'"' | b'\'')) {
            i += 1;
        }
        let close = skip_ws(b, i);
        if b.get(close) == Some(&b']') {
            i = close + 1;
        }
        i = skip_ws(b, i);
        if !matches!(b.get(i), Some(b':' | b'=')) {
            return None;
        }
        i = skip_ws(b, i + 1);
        let end = number_end(b, i)?;
        let number = normalize(&body[i..end]);
        i = skip_ws(b, end);
        if matches!(b.get(i), Some(b',' | b';')) {
            i += 1;
        }
        i = skip_ws(b, i);
        if matches!(key.as_str(), "w" | "h" | "heading") {
            key = "w".into();
        }
        if !KEYS.contains(&key.as_str()) || values.iter().any(|(k, _)| *k == key) {
            return None;
        }
        values.push((key, number));
    }
    let get = |k: &str| values.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
    let mut components = vec![get("x")?, get("y")?, get("z")?];
    components.extend(get("w"));
    Some(CoordText { components, format: Format::Keyed, lua_vector: false })
}

fn parse_list(text: &str) -> Option<CoordText> {
    let mut body = text;
    if (body.starts_with('[') && body.ends_with(']')) || (body.starts_with('(') && body.ends_with(')')) {
        body = body[1..body.len() - 1].trim();
    }
    let numbers = list(body, true).or_else(|| list(body, false))?;
    // Integers alone (`1, 2, 3`) are arguments, indices, colours: not clearly coordinates.
    if numbers.iter().take(2).any(|n| !n.contains('.')) {
        return None;
    }
    Some(CoordText {
        components: numbers.into_iter().map(normalize).collect(),
        format: Format::List,
        lua_vector: false,
    })
}

fn list(s: &str, commas: bool) -> Option<Vec<&str>> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        let end = number_end(b, i)?;
        out.push(&s[i..end]);
        if end == b.len() {
            return (out.len() >= 3).then_some(out);
        }
        if out.len() == 4 {
            return None;
        }
        let next = skip_ws(b, end);
        i = if commas {
            if b.get(next) != Some(&b',') {
                return None;
            }
            skip_ws(b, next + 1)
        } else {
            if next == end {
                return None;
            }
            next
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vector_of(text: &str) -> Option<String> {
        parse(text).map(|c| c.to_vector())
    }

    #[test]
    fn coordinates() {
        assert_eq!(vector_of("123.4, 56.7, 8.9").as_deref(), Some("vector3(123.4, 56.7, 8.9)"));
        assert_eq!(vector_of("123.4, 56.7, 8.9, 90.0").as_deref(), Some("vector4(123.4, 56.7, 8.9, 90.0)"));
        assert_eq!(vector_of("  -1037.74,-2738.03,20.17,\n").as_deref(), Some("vector3(-1037.74, -2738.03, 20.17)"));
        assert_eq!(vector_of("1.5 2.5 3").as_deref(), Some("vector3(1.5, 2.5, 3)"));
        assert_eq!(vector_of("[1.5, 2.5, 3.5]").as_deref(), Some("vector3(1.5, 2.5, 3.5)"));
        assert_eq!(vector_of("(1.5, 2.5, 3.5)").as_deref(), Some("vector3(1.5, 2.5, 3.5)"));
        let vec4 = parse("vec4(1.0,2.0,3.0,4.0)").unwrap();
        assert!(vec4.lua_vector);
        assert_eq!(vec4.to_vector(), "vector4(1.0, 2.0, 3.0, 4.0)");
        assert!(!parse("Vector3(1.0, 2.0, 3.0)").unwrap().lua_vector);
        assert_eq!(vector_of("new Vector3(1f, 2f, 3.5f)").as_deref(), Some("vector3(1, 2, 3.5)"));
        for keyed in [
            "{x = 1.0, y = 2.0, z = 3.0}",
            "{ ['x'] = 1.0, ['y'] = 2.0, ['z'] = 3.0 }",
            "x: 1.0 y: 2.0 z: 3.0",
            "x=1.0 y=2.0 z=3.0",
        ] {
            assert_eq!(vector_of(keyed).as_deref(), Some("vector3(1.0, 2.0, 3.0)"), "{keyed}");
        }
        assert_eq!(
            vector_of("{\"x\":1.5,\"y\":-2.5,\"z\":3.5,\"heading\":270.0}").as_deref(),
            Some("vector4(1.5, -2.5, 3.5, 270.0)")
        );
        assert_eq!(
            vector_of("{\"z\": 3.5, \"w\": 270.0, \"y\": -2.5, \"x\": 1.5}").as_deref(),
            Some("vector4(1.5, -2.5, 3.5, 270.0)")
        );
        assert_eq!(vector_of("X: 1.0, Y: 2.0, Z: 3.0, H: 45.0").as_deref(), Some("vector4(1.0, 2.0, 3.0, 45.0)"));
        assert_eq!(parse("vector3(1,2,3)").map(|c| c.format), Some(Format::Vector));
    }

    #[test]
    fn not_coordinates() {
        for text in [
            "",
            "hello",
            "1, 2, 3",
            "1.5, 2",
            "1.5, 2.5, 3.5, 4.5, 5.5",
            "1.5, 2.5, 3.5 -- comment",
            "{x = 1.0, y = 2.0}",
            "{x = 1.0, y = 2.0, z = 3.0, name = 4.0}",
            "{x = 1.0, x = 2.0, z = 3.0}",
            "x: 1.0 y: 2.0 z: 3.0 w: 4.0 h: 5.0",
            "{1.5, 2.5, 3.5}",
            "print(1.5, 2.5, 3.5)",
            "a, b, c",
            "vector3(x, y, z)",
            "vector3(1.0, 2.0, 3.0, 4.0)",
            "vector2(1.0, 2.0)",
        ] {
            assert_eq!(parse(text), None, "{text}");
        }
    }

    #[test]
    fn builders() {
        assert_eq!(keyed(&["a", "b", "c", "d"]), "{ x = a, y = b, z = c, w = d }");
        assert_eq!(array(&["a", "b", "c"]), "{ a, b, c }");
        assert_eq!(vector("vec4", &["a", "b", "c", "d"]), "vec4(a, b, c, d)");
    }
}
