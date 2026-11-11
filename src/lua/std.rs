const FUNCTIONS: &[&str] = &[
    "assert",
    "collectgarbage",
    "dofile",
    "error",
    "getmetatable",
    "ipairs",
    "load",
    "loadfile",
    "next",
    "pairs",
    "pcall",
    "print",
    "rawequal",
    "rawget",
    "rawlen",
    "rawset",
    "require",
    "select",
    "setmetatable",
    "tonumber",
    "tostring",
    "type",
    "warn",
    "xpcall",
];

const VALUES: &[&str] =
    &["_G", "_VERSION", "coroutine", "debug", "io", "math", "os", "package", "string", "table", "utf8"];

/// Whether `name` is a global of the Lua 5.4 standard library, and if so whether it is a function.
pub fn global(name: &str) -> Option<bool> {
    if FUNCTIONS.contains(&name) {
        return Some(true);
    }
    VALUES.contains(&name).then_some(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn functions_and_library_tables() {
        assert_eq!(global("print"), Some(true));
        assert_eq!(global("string"), Some(false));
        assert_eq!(global("_G"), Some(false));
        assert_eq!(global("Citizen"), None);
    }
}
