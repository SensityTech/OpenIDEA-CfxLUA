// Installs a plugin package with OpenIDEA's own plugin host and runs one of its commands, which instantiates and
// activates the component: `openidea-plugin check` only compares interface names and versions, not their types.
// Built as an example of OpenIDEA's `ide-host` crate by .github/workflows/openidea-compat.yml:
//
//   cp scripts/openidea-start-check.rs <openidea>/crates/ide-host/examples/cfxlua_start_check.rs
//   cargo run --release --locked -p ide-host --example cfxlua_start_check -- <package.oiplugin> <command>

use ide_host::plugins::{Consent, PluginHost, PluginStatus};
use ide_plugin::{Invocation, Limits};
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [package, command] = args.as_slice() else {
        eprintln!("usage: cfxlua_start_check <package.oiplugin> <command>");
        return ExitCode::from(2);
    };
    let dir = std::env::temp_dir().join(format!("cfxlua-start-check-{}", std::process::id()));
    let result = start(&dir, Path::new(package), command);
    let _ = std::fs::remove_dir_all(&dir);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            println!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn start(dir: &Path, package: &Path, command: &str) -> Result<(), String> {
    let host = PluginHost::new(dir.to_path_buf(), Arc::new(|_: Option<&str>, _| {}), Limits::default())
        .map_err(|e| e.to_string())?;
    let unpacked = host.unpack_package(package).map_err(|e| e.to_string())?;
    let installed = host.install_from_dir(&unpacked, &Consent::all()).map_err(|e| e.to_string())?;
    let outcome = host.run_command(&installed.id, command, None, Invocation::default());
    let info = host.info_of(&installed.id).map_err(|e| e.to_string())?;
    println!(
        "{} {}: {:?}; {command}: {:?}",
        info.id,
        info.version,
        info.status,
        outcome.map_err(|e| e.to_string())
    );
    match (info.status, info.error) {
        (PluginStatus::Active, None) => Ok(()),
        (status, error) => Err(format!("the plugin does not run: {status:?} {}", error.unwrap_or_default())),
    }
}
