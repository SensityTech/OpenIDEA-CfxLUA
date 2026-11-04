//! The manifest live templates of the JetBrains plugin (`fxm`, `cls`, `nui`…),
//! offered as completion items at statement start: the IDE inserts plain
//! text, so every variable takes its default value.

use super::catalog::RDR3_WARNING;

/// One template: abbreviation, description, text (`$NAME$` variables, `$END$` caret).
pub struct Snippet {
    pub abbrev: &'static str,
    pub description: &'static str,
    text: &'static str,
    /// Variable defaults (unlisted variables are empty).
    defaults: &'static [(&'static str, &'static str)],
}

const HEADER: &str = "author '$AUTHOR$'\ndescription '$DESCRIPTION$'\nversion '1.0.0'\n\n$END$";

pub const SNIPPETS: &[Snippet] = &[
    Snippet {
        abbrev: "fxm",
        description: "fxmanifest.lua header for FiveM",
        text: "fx_version 'cerulean'\ngame 'gta5'\n\n$HEADER$",
        defaults: &[],
    },
    Snippet {
        abbrev: "fxmr",
        description: "fxmanifest.lua header for RedM",
        text: "fx_version 'cerulean'\ngame 'rdr3'\nrdr3_warning '$RDR3_WARNING$'\n\n$HEADER$",
        defaults: &[],
    },
    Snippet {
        abbrev: "fxmb",
        description: "fxmanifest.lua header for FiveM and RedM",
        text: "fx_version 'cerulean'\ngames { 'gta5', 'rdr3' }\nrdr3_warning '$RDR3_WARNING$'\n\n$HEADER$",
        defaults: &[],
    },
    Snippet {
        abbrev: "shs",
        description: "shared_scripts { ... } (loaded first on both sides)",
        text: "shared_scripts {\n    '$PATH$',\n}\n$END$",
        defaults: &[("PATH", "shared/*.lua")],
    },
    Snippet {
        abbrev: "cls",
        description: "client_scripts { ... }",
        text: "client_scripts {\n    '$PATH$',\n}\n$END$",
        defaults: &[("PATH", "client/*.lua")],
    },
    Snippet {
        abbrev: "svs",
        description: "server_scripts { ... }",
        text: "server_scripts {\n    '$PATH$',\n}\n$END$",
        defaults: &[("PATH", "server/*.lua")],
    },
    Snippet { abbrev: "fls", description: "files { ... } (files downloaded by clients)", text: "files {\n    '$PATH$',\n}\n$END$", defaults: &[] },
    Snippet {
        abbrev: "nui",
        description: "ui_page and its files",
        text: "ui_page '$DIR$/index.html'\n\nfiles {\n    '$DIR$/index.html',\n    '$DIR$/*.css',\n    '$DIR$/*.js',\n}\n$END$",
        defaults: &[("DIR", "web")],
    },
    Snippet {
        abbrev: "deps",
        description: "dependencies { ... } (resources and constraints)",
        text: "dependencies {\n    '$DEPENDENCY$',\n}\n$END$",
        defaults: &[],
    },
    Snippet {
        abbrev: "dataf",
        description: "data_file 'TYPE' 'path' with its files entry",
        text: "files {\n    '$PATH$',\n}\n\ndata_file '$TYPE$' '$PATH$'\n$END$",
        defaults: &[("PATH", "data/*.meta"), ("TYPE", "VEHICLE_METADATA_FILE")],
    },
    Snippet {
        abbrev: "escrow",
        description: "escrow_ignore { ... } (files left unencrypted)",
        text: "escrow_ignore {\n    '$PATH$',\n}\n$END$",
        defaults: &[("PATH", "config.lua")],
    },
];

impl Snippet {
    /// The text with every variable at its default, `$END$` removed and the
    /// lines after the first indented with `indent`.
    pub fn expand(&self, indent: &str) -> String {
        let mut text = self.text.replace("$HEADER$", HEADER).replace("$RDR3_WARNING$", RDR3_WARNING).replace("$END$", "");
        for (name, value) in self.defaults {
            text = text.replace(&format!("${name}$"), value);
        }
        // Variables without a default (`$AUTHOR$`, `$PATH$` of `fls`) are empty.
        while let Some(start) = text.find('$') {
            let Some(len) = text[start + 1..].find('$') else { break };
            text.replace_range(start..start + len + 2, "");
        }
        if indent.is_empty() {
            return text;
        }
        text.split('\n')
            .enumerate()
            .map(|(i, l)| if i == 0 || l.is_empty() { l.to_owned() } else { format!("{indent}{l}") })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(abbrev: &str) -> &'static Snippet {
        SNIPPETS.iter().find(|s| s.abbrev == abbrev).unwrap()
    }

    #[test]
    fn expansions_use_the_defaults() {
        assert_eq!(
            get("fxmr").expand(""),
            format!("fx_version 'cerulean'\ngame 'rdr3'\nrdr3_warning '{RDR3_WARNING}'\n\nauthor ''\ndescription ''\nversion '1.0.0'\n\n")
        );
        assert_eq!(get("nui").expand(""), "ui_page 'web/index.html'\n\nfiles {\n    'web/index.html',\n    'web/*.css',\n    'web/*.js',\n}\n");
        assert_eq!(get("dataf").expand(""), "files {\n    'data/*.meta',\n}\n\ndata_file 'VEHICLE_METADATA_FILE' 'data/*.meta'\n");
        assert_eq!(get("fls").expand(""), "files {\n    '',\n}\n");
        assert_eq!(get("cls").expand("  "), "client_scripts {\n      'client/*.lua',\n  }\n");
        assert_eq!(SNIPPETS.len(), 11);
    }
}
