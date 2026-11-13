use crate::nui::build::{dir_exists, join};
use crate::project::Game;
use crate::text::{file_name, parent, slash};
use crate::workspace::Workspace;

const DEFAULT_CONFIG: &str = "server.cfg";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchSettings {
    /// Relative to the project root; detected from the project when blank.
    pub data_folder: String,
    /// Relative to the server data folder.
    pub config: String,
    pub game: Game,
    pub arguments: String,
}

/// The arguments of FXServer started at the project root, or the first problem of `settings`.
pub fn command_line(ws: &Workspace, settings: &LaunchSettings) -> Result<Vec<String>, String> {
    let (data, shown) = data_folder(ws, &settings.data_folder)?;
    let config = settings.config.trim();
    if config.is_empty() {
        return Err("Specify the config file executed at startup (server.cfg)".to_owned());
    }
    let path = join(&data, &slash(config)).filter(|p| ws.exists(p));
    let Some(path) = path else {
        return Err(format!("Config file '{}' not found in '{shown}'", settings.config));
    };
    let mut args = Vec::new();
    if settings.game == Game::Rdr3 {
        args.extend(["+set", "gamename", Game::Rdr3.manifest_name()].map(str::to_owned));
    }
    args.push("+exec".to_owned());
    args.push(ws.relative(&path).to_owned());
    args.extend(split_arguments(&settings.arguments));
    Ok(args)
}

fn data_folder(ws: &Workspace, setting: &str) -> Result<(String, String), String> {
    let setting = setting.trim();
    if setting.is_empty() {
        let detected = data_folders(ws).into_iter().next();
        let Some(dir) = detected else {
            return Err("Specify the server data folder (the folder holding server.cfg and resources/)".to_owned());
        };
        let shown = match ws.relative(&dir) {
            "" => ".".to_owned(),
            relative => relative.to_owned(),
        };
        return Ok((dir, shown));
    }
    let relative = slash(setting);
    let dir = join(&ws.root, ws.relative(&relative)).filter(|d| *d == ws.root || dir_exists(ws, d));
    match dir {
        Some(dir) => Ok((dir, setting.to_owned())),
        None => Err(format!("Server data folder '{setting}' not found")),
    }
}

fn data_folders(ws: &Workspace) -> Vec<String> {
    let mut dirs: Vec<String> =
        ws.files().filter(|f| file_name(&f.path) == DEFAULT_CONFIG).map(|f| parent(&f.path).to_owned()).collect();
    dirs.sort_by_cached_key(|d| (!dir_exists(ws, &format!("{d}/resources")), d.matches('/').count(), d.clone()));
    dirs
}

fn split_arguments(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut token = String::new();
    let mut quoted = false;
    let mut started = false;
    let mut chars = text.trim().chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if chars.peek() == Some(&'"') => {
                token.push('"');
                chars.next();
            }
            c if c.is_whitespace() && !quoted => {
                if started || !token.is_empty() {
                    out.push(std::mem::take(&mut token));
                    started = false;
                }
            }
            c => token.push(c),
        }
    }
    if started || !token.is_empty() {
        out.push(token);
    }
    out
}
