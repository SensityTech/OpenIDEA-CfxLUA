#[derive(Clone, Debug, PartialEq, Eq)]
enum Part {
    AnyDirs,
    AnyDeep,
    Any,
    One,
    Lit(char),
}

#[derive(Clone, Debug)]
pub struct Glob {
    parts: Vec<Part>,
    case_sensitive: bool,
}

impl Glob {
    pub fn new(pattern: &str, case_sensitive: bool) -> Option<Self> {
        let mut g = pattern.trim().replace('\\', "/");
        if g.is_empty() || g.starts_with('@') {
            return None;
        }
        while let Some(rest) = g.strip_prefix("./") {
            g = rest.to_owned();
        }
        let g = g.trim_start_matches('/');
        if g.is_empty() {
            return None;
        }
        let chars: Vec<char> = g.chars().collect();
        let mut parts = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '*' && chars.get(i + 1) == Some(&'*') && (i == 0 || chars[i - 1] == '/') {
                i += 2;
                if chars.get(i) == Some(&'/') {
                    parts.push(Part::AnyDirs);
                    i += 1;
                } else {
                    parts.push(Part::AnyDeep);
                }
                continue;
            }
            parts.push(match c {
                '*' => Part::Any,
                '?' => Part::One,
                _ => Part::Lit(if case_sensitive { c } else { c.to_ascii_lowercase() }),
            });
            i += 1;
        }
        Some(Self { parts, case_sensitive })
    }

    pub fn matches(&self, rel: &str) -> bool {
        let s: Vec<char> = if self.case_sensitive {
            rel.chars().collect()
        } else {
            rel.chars().map(|c| c.to_ascii_lowercase()).collect()
        };
        matches_at(&self.parts, &s)
    }

    pub fn literal_prefix(&self) -> String {
        let mut out = String::new();
        for p in &self.parts {
            match p {
                Part::Lit(c) => out.push(*c),
                _ => break,
            }
        }
        match out.rfind('/') {
            Some(i) => out[..=i].to_owned(),
            None => String::new(),
        }
    }

    pub fn has_wildcards(&self) -> bool {
        self.parts.iter().any(|p| !matches!(p, Part::Lit(_)))
    }
}

fn matches_at(p: &[Part], s: &[char]) -> bool {
    match p.first() {
        None => s.is_empty(),
        Some(Part::Lit(c)) => s.first() == Some(c) && matches_at(&p[1..], &s[1..]),
        Some(Part::One) => s.first().is_some_and(|c| *c != '/') && matches_at(&p[1..], &s[1..]),
        Some(Part::Any) => {
            for n in 0..=s.len() {
                if matches_at(&p[1..], &s[n..]) {
                    return true;
                }
                if s.get(n) == Some(&'/') {
                    break;
                }
            }
            false
        }
        Some(Part::AnyDeep) => (0..=s.len()).any(|n| matches_at(&p[1..], &s[n..])),
        Some(Part::AnyDirs) => {
            // Nothing, or whole directories (`a/`, `a/b/`).
            if matches_at(&p[1..], s) {
                return true;
            }
            (0..s.len()).filter(|&n| s[n] == '/').any(|n| matches_at(&p[1..], &s[n + 1..]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(p: &str, s: &str) -> bool {
        Glob::new(p, true).is_some_and(|g| g.matches(s))
    }

    #[test]
    fn fxserver_matrix() {
        assert!(m("client/*.lua", "client/main.lua"));
        assert!(!m("client/*.lua", "client/sub/main.lua"));
        assert!(m("client/**.lua", "client/main.lua"));
        assert!(m("client/**.lua", "client/sub/main.lua"));
        assert!(m("**/*.lua", "main.lua"));
        assert!(m("**/*.lua", "a/b/main.lua"));
        assert!(m("src/**/cl_*.lua", "src/cl_main.lua"));
        assert!(m("src/**/cl_*.lua", "src/a/b/cl_main.lua"));
        assert!(!m("src/**/cl_*.lua", "src/a/sv_main.lua"));
        assert!(m("lib**.lua", "library.lua"));
        assert!(!m("lib**.lua", "lib/x.lua"));
        assert!(m("./config.lua", "config.lua"));
        assert!(m("data\\items.lua", "data/items.lua"));
        assert!(!m("config.lua", "configXlua"));
        assert!(!m("config.lua", "Config.lua"));
        assert!(Glob::new("config.lua", false).unwrap().matches("Config.lua"));
        assert!(Glob::new("@ox_lib/init.lua", true).is_none());
        assert!(Glob::new("  ", true).is_none());
        assert_eq!(Glob::new("client/**/*.lua", true).unwrap().literal_prefix(), "client/");
    }
}
