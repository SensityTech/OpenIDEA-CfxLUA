use crate::manifest::catalog::{FX_VERSION_LINE, rdr3_warning_line};
use crate::project::{FX_MANIFEST, Game};

pub const TITLE: &str = "New CfxLua Resource";
pub const NAME_COMMENT: &str = "The folder name is the resource name";
pub const GAME_COMMENT: &str = "RedM resources also get the rdr3_warning acknowledgement";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Target {
    #[default]
    FiveM,
    RedM,
    Both,
}

impl Target {
    pub const ALL: [Target; 3] = [Target::FiveM, Target::RedM, Target::Both];

    pub fn label(self) -> &'static str {
        match self {
            Target::FiveM => "FiveM (gta5)",
            Target::RedM => "RedM (rdr3)",
            Target::Both => "FiveM and RedM",
        }
    }

    pub fn of(games: &[Game]) -> Self {
        match (games.contains(&Game::Gta5), games.contains(&Game::Rdr3)) {
            (true, true) => Target::Both,
            (false, true) => Target::RedM,
            _ => Target::FiveM,
        }
    }

    fn rdr3(self) -> bool {
        self != Target::FiveM
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Script {
    Client,
    Server,
    Shared,
    Nui,
}

impl Script {
    pub const ALL: [Script; 4] = [Script::Client, Script::Server, Script::Shared, Script::Nui];
    pub const DEFAULTS: [Script; 3] = [Script::Client, Script::Server, Script::Shared];

    pub fn label(self) -> &'static str {
        match self {
            Script::Client => "Client",
            Script::Server => "Server",
            Script::Shared => "Shared config",
            Script::Nui => "NUI page",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub name: String,
    pub target: Target,
    pub client: bool,
    pub server: bool,
    pub shared: bool,
    pub nui: bool,
    pub author: String,
    pub description: String,
}

impl Options {
    pub fn new(name: &str, target: Target) -> Self {
        Self {
            name: name.to_owned(),
            target,
            client: true,
            server: true,
            shared: false,
            nui: false,
            author: String::new(),
            description: String::new(),
        }
    }

    /// What the dialog answers gave: trimmed texts, and a NUI page that implies the client script.
    pub fn from_answers(name: &str, target: Target, author: &str, description: &str, scripts: &[Script]) -> Self {
        let nui = scripts.contains(&Script::Nui);
        Self {
            name: name.trim().to_owned(),
            target,
            client: scripts.contains(&Script::Client) || nui,
            server: scripts.contains(&Script::Server),
            shared: scripts.contains(&Script::Shared),
            nui,
            author: author.trim().to_owned(),
            description: description.trim().to_owned(),
        }
    }
}

/// `exists` is asked about the trimmed name, a child of the folder the resource goes in.
pub fn validate_name(name: &str, exists: impl FnOnce(&str) -> bool) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Some("Enter the resource name".to_owned());
    }
    if !trimmed.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')) {
        return Some("Use letters, digits, '_', '-' and '.' only: the folder name is the resource name".to_owned());
    }
    if exists(trimmed) {
        return Some(format!("'{trimmed}' already exists"));
    }
    None
}

pub fn files(options: &Options) -> Vec<(&'static str, String)> {
    let mut files = vec![(FX_MANIFEST, manifest(options))];
    if options.shared {
        files.push(("shared/config.lua", SHARED_CONFIG.to_owned()));
    }
    if options.client || options.nui {
        files.push(("client/main.lua", client_script(options)));
    }
    if options.server {
        files.push(("server/main.lua", server_script(options)));
    }
    if options.nui {
        files.push(("web/index.html", nui_page(options)));
        files.push(("web/style.css", NUI_STYLE.to_owned()));
        files.push(("web/script.js", NUI_SCRIPT.to_owned()));
    }
    files
}

pub fn manifest(options: &Options) -> String {
    let mut out = format!("{FX_VERSION_LINE}\n");
    out.push_str(match options.target {
        Target::FiveM => "game 'gta5'\n",
        Target::RedM => "game 'rdr3'\n",
        Target::Both => "games { 'gta5', 'rdr3' }\n",
    });
    if options.target.rdr3() {
        out.push_str(&rdr3_warning_line());
        out.push('\n');
    }
    out.push('\n');
    if !options.author.trim().is_empty() {
        out.push_str(&format!("author {}\n", quote(&options.author)));
    }
    let description = if options.description.trim().is_empty() { &options.name } else { &options.description };
    out.push_str(&format!("description {}\n", quote(description)));
    out.push_str("version '1.0.0'\n");
    if options.shared {
        table(&mut out, "shared_scripts", &["shared/*.lua"]);
    }
    if options.client || options.nui {
        table(&mut out, "client_scripts", &["client/*.lua"]);
    }
    if options.server {
        table(&mut out, "server_scripts", &["server/*.lua"]);
    }
    if options.nui {
        out.push_str("\nui_page 'web/index.html'\n");
        table(&mut out, "files", &["web/index.html", "web/style.css", "web/script.js"]);
    }
    out
}

fn table(out: &mut String, directive: &str, values: &[&str]) {
    out.push_str(&format!("\n{directive} {{\n"));
    for value in values {
        out.push_str(&format!("    '{value}',\n"));
    }
    out.push_str("}\n");
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn client_script(options: &Options) -> String {
    let mut out = format!(
        "-- Client script of {}\n\nCreateThread(function()\n    print(('[%s] client started'):format(GetCurrentResourceName()))\nend)\n",
        options.name
    );
    if options.nui {
        out.push_str(&format!(
            r"
local visible = false

--- Shows or hides the NUI page and gives it the mouse and keyboard focus.
---@param state boolean
local function setVisible(state)
    visible = state
    SetNuiFocus(state, state)
    SendNUIMessage({{ action = 'setVisible', visible = state }})
end

RegisterCommand({}, function()
    setVisible(not visible)
end, false)

-- web/script.js posts to https://<resource>/close when the page is closed
RegisterNUICallback('close', function(_, cb)
    setVisible(false)
    cb({{}})
end)
",
            quote(&options.name)
        ));
    }
    out
}

fn server_script(options: &Options) -> String {
    format!(
        r"-- Server script of {}

AddEventHandler('onResourceStart', function(resourceName)
    if resourceName ~= GetCurrentResourceName() then
        return
    end
    print(('[%s] started'):format(resourceName))
end)
",
        options.name
    )
}

const SHARED_CONFIG: &str = r"-- Shared configuration: loaded first, on the client and on the server
Config = {}

Config.Debug = false
";

fn nui_page(options: &Options) -> String {
    let name = &options.name;
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>{name}</title>
    <link rel="stylesheet" href="style.css">
</head>
<body>
    <div id="app" class="hidden">
        <h1>{name}</h1>
        <button id="close">Close</button>
    </div>
    <script src="script.js"></script>
</body>
</html>
"#
    )
}

const NUI_STYLE: &str = r"body {
    margin: 0;
    background: transparent;
    font-family: sans-serif;
}

.hidden {
    display: none;
}

#app {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    padding: 24px;
    border-radius: 8px;
    background: rgba(20, 20, 20, 0.9);
    color: white;
}
";

const NUI_SCRIPT: &str = r"const app = document.getElementById('app');

// messages sent by SendNUIMessage in client/main.lua
window.addEventListener('message', (event) => {
    const data = event.data;
    if (data.action === 'setVisible') {
        app.classList.toggle('hidden', !data.visible);
    }
});

// NUI callbacks use https:// with fx_version 'cerulean'
function close() {
    fetch(`https://${GetParentResourceName()}/close`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json; charset=UTF-8' },
        body: JSON.stringify({}),
    });
}

document.getElementById('close').addEventListener('click', close);
document.addEventListener('keyup', (event) => {
    if (event.key === 'Escape') {
        close();
    }
});
";

#[cfg(test)]
#[path = "generate_tests.rs"]
mod tests;
