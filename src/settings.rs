//! The plugin's settings (Settings › Plugins › Sensity DevKit, per project or
//! global), declared in `settings.schema.json`. Features read them through
//! [`Settings`] with the schema's default, so a missing or mistyped value never
//! breaks anything; tests set values with [`Settings::with`].

use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Default)]
pub struct Settings {
    /// Values set by tests (or read from the IDE), by key.
    values: RefCell<HashMap<String, Option<Value>>>,
    /// Read the IDE's settings on first use of a key (false in tests).
    live: bool,
}

impl Settings {
    /// Settings read from the IDE (cached until [`Settings::reset`]).
    pub fn live() -> Self {
        Self { values: RefCell::default(), live: true }
    }

    /// Defaults only (tests), plus what [`Settings::with`] sets.
    pub fn defaults() -> Self {
        Self::default()
    }

    pub fn with(self, key: &str, value: Value) -> Self {
        self.values.borrow_mut().insert(key.to_owned(), Some(value));
        self
    }

    /// Forgets cached values (the user may have changed them).
    pub fn reset(&self) {
        if self.live {
            self.values.borrow_mut().clear();
        }
    }

    fn get(&self, key: &str) -> Option<Value> {
        if let Some(v) = self.values.borrow().get(key) {
            return v.clone();
        }
        let v = if self.live {
            openidea_plugin::host::setting(key).and_then(|s| serde_json::from_str(&s).ok())
        } else {
            None
        };
        self.values.borrow_mut().insert(key.to_owned(), v.clone());
        v
    }

    pub fn bool(&self, key: &str, default: bool) -> bool {
        self.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
    }

    pub fn string(&self, key: &str, default: &str) -> String {
        self.get(key).and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_else(|| default.to_owned())
    }

    pub fn number(&self, key: &str, default: f64) -> f64 {
        self.get(key).and_then(|v| v.as_f64()).unwrap_or(default)
    }

    pub fn int(&self, key: &str, default: i64) -> i64 {
        self.get(key).and_then(|v| v.as_i64()).unwrap_or(default)
    }

    /// A list written as text: items separated by commas, semicolons, spaces or new lines.
    pub fn list(&self, key: &str, default: &str) -> Vec<String> {
        let raw = self.string(key, default);
        let mut out: Vec<String> = Vec::new();
        for item in raw.split([',', ';', ' ', '\n', '\t']).map(str::trim).filter(|s| !s.is_empty()) {
            if !out.iter().any(|o| o == item) {
                out.push(item.to_owned());
            }
        }
        out
    }

    /// Master switch: every FiveM feature is off when false.
    pub fn enabled(&self) -> bool {
        self.bool(keys::ENABLED, true)
    }
}

/// Keys shared by several features. Feature-specific keys live in their module.
pub mod keys {
    pub const ENABLED: &str = "enabled";
    /// `auto` (from the manifests), `fivem`, `redm`, `both`.
    pub const GAME: &str = "game";
    pub const CLIENT_NATIVES: &str = "clientNatives";
    pub const SERVER_NATIVES: &str = "serverNatives";
    pub const RUNTIME_API: &str = "runtimeApi";
    pub const SHOW_DEPRECATED: &str = "showDeprecatedAliasesInCompletion";
    pub const SHOW_UNNAMED: &str = "showUnnamedNativesInCompletion";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_with_defaults() {
        let s = Settings::defaults().with("nui.messageKeys", Value::from("action; type, action\nkind"));
        assert_eq!(s.list("nui.messageKeys", ""), ["action", "type", "kind"]);
        assert!(s.enabled());
        assert_eq!(s.string("game", "auto"), "auto");
        let s = s.with("enabled", Value::from("yes"));
        assert!(s.enabled(), "a mistyped value falls back to the default");
    }
}
