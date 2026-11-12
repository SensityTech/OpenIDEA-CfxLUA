//! How a name answers what is typed of it: the whole name, its start, or the starts of its words.

const MAX_HUMPS: usize = 48;

/// The better matches come first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Match {
    Exact,
    Prefix,
    CamelHump,
}

/// What is typed of a name, compared without case.
pub struct Query {
    lower: String,
    /// Without its underscores: the letters the words of a name start with.
    compact: Vec<u8>,
}

impl Query {
    pub fn new(typed: &str) -> Self {
        let lower = typed.to_ascii_lowercase();
        let compact = lower.bytes().filter(|&b| b != b'_').collect();
        Self { lower, compact }
    }

    /// How `name` answers the query: nothing typed is the start of every name.
    pub fn matches(&self, name: &str) -> Option<Match> {
        let (n, q) = (name.as_bytes(), self.lower.as_bytes());
        if n.len() >= q.len() && n[..q.len()].eq_ignore_ascii_case(q) {
            return Some(if n.len() == q.len() { Match::Exact } else { Match::Prefix });
        }
        let first = n.iter().find(|&&b| b != b'_')?;
        let compact = &self.compact;
        if compact.is_empty() || compact.len() > MAX_HUMPS || first.to_ascii_lowercase() != compact[0] {
            return None;
        }
        camel_humps(&words(name), compact, 0, 0, true).then_some(Match::CamelHump)
    }
}

/// The words of a name: `GetGroundZFor_3dCoord` is `Get`, `Ground`, `ZFor`, `3d`, `Coord`.
pub fn words(name: &str) -> Vec<&str> {
    let b = name.as_bytes();
    let mut out = Vec::new();
    let (mut start, mut i) = (0, 0);
    while i < b.len() {
        if b[i] == b'_' {
            if i > start {
                out.push(&name[start..i]);
            }
            while i < b.len() && b[i] == b'_' {
                i += 1;
            }
            start = i;
            continue;
        }
        if i > start {
            let (p, c) = (b[i - 1], b[i]);
            let lower_to_upper = (p.is_ascii_lowercase() || p.is_ascii_digit()) && c.is_ascii_uppercase();
            if lower_to_upper || (p.is_ascii_alphabetic() && c.is_ascii_digit()) {
                out.push(&name[start..i]);
                start = i;
            }
        }
        i += 1;
    }
    if start < b.len() {
        out.push(&name[start..]);
    }
    out
}

fn camel_humps(words: &[&str], q: &[u8], qi: usize, wi: usize, anchored: bool) -> bool {
    if qi == q.len() {
        return true;
    }
    let Some(max) = words.len().checked_sub(1) else { return false };
    let last = if anchored { wi.min(max) } else { max };
    for w in wi..=last {
        let word = words[w].as_bytes();
        if word[0].to_ascii_lowercase() != q[qi] {
            continue;
        }
        let mut common = 1;
        while common < word.len() && qi + common < q.len() && word[common].to_ascii_lowercase() == q[qi + common] {
            common += 1;
        }
        if (1..=common).rev().any(|len| camel_humps(words, q, qi + len, w + 1, false)) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_answers_by_its_start_or_by_the_starts_of_its_words() {
        assert_eq!(words("GetGroundZFor_3dCoord"), ["Get", "Ground", "ZFor", "3d", "Coord"]);
        for (typed, name) in
            [("ggzf3c", "GetGroundZFor_3dCoord"), ("getgrz", "GetGroundZFor_3dCoord"), ("gcoords", "GetEntityCoords")]
        {
            assert_eq!(Query::new(typed).matches(name), Some(Match::CamelHump), "{typed}");
        }
        assert_eq!(Query::new("gec").matches("GetEntityHeading"), None);
        assert_eq!(Query::new("ec").matches("GetEntityCoords"), None, "the first word is anchored");
        assert_eq!(Query::new("GETENTITYC").matches("GetEntityCoords"), Some(Match::Prefix), "without case");
        assert_eq!(Query::new("print").matches("print"), Some(Match::Exact));
        assert_eq!(Query::new("").matches("print"), Some(Match::Prefix), "nothing typed starts every name");
        assert_eq!(Query::new("").matches(""), Some(Match::Exact));
        assert_eq!(Query::new("_").matches("_G"), Some(Match::Prefix));
        assert_eq!(Query::new("x").matches("___"), None, "a name without a letter");
        assert!(Match::Exact < Match::Prefix && Match::Prefix < Match::CamelHump);
    }
}
